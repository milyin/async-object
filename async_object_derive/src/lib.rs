//! # Async Object Derive
//!
//! This library provides set of procedural macros to make convenient reference-counting wrappers for using objects in asynchronous environment.
//!
//! # Example
//!
//! This code makes wrappers Background and WBackground for BackgroundImpl object. Internally they are just
//! Arc<Rwlock<BackgroundImpl>> and Weak<Rwlock<BackgroundImpl>> plus tooling for access the Rwlock without blocking
//! asyncronous job.
//!
//!  ```compile fail
//! #[async_object_decl(pub Background, pub WBackground)]
//! struct BackgroundImpl {
//!     color: Color
//! }
//!
//! #[async_object_impl(Background,  WBackground)]
//! impl BackgroundImpl {
//!     pub fn set_color(&mut this, color: Color) {
//!         this.color = color
//!     }
//!     pub fn get_color(&this) -> Color {
//!         this.color
//!     }
//! }
//!
//! impl Background {
//!     pub fn new() -> Background {
//!         Background::create(BackgroundImpl { color: Color::White })
//!     }
//! }
//! ```
//!
//! The structures Background and WBackground will have these automatically generated proxy methods:
//!
//! ```compile fail
//! impl Background {
//!     pub fn set_color(...);
//!     pub fn get_golor() -> Color;
//!     async pub fn async_set_color(...);
//!     async pub fn async_get_color(...) -> Color
//! }
//!
//! impl WBackground {
//!     pub fn set_color(...) -> Option<()>;
//!     pub fn get_golor() -> Option<Color>;
//!     async pub fn async_set_color(...) -> Option<()>;
//!     async pub fn async_get_color(...) -> Option<Color>
//! }
//!  ```
//! Note Option return type for weak wrapper WBackground. If all instance of Background are destroyed,
//! the BackgroundImpl is dropped too. Remaining WBackground instances starts to return None for all method calls.
//!
//! The difference between normal and async proxy methods is in their way to mutex access. The synchronous methods are just
//! waiting for mutex unlock. The async methods tries to access the mutex and if it's locked puts asynchronous task to sleep
//! until mutex is released, allowing other tasks to continue on this worker.
//!  
//! There is also event pub/sub support. For example:
//! ```compile fail
//! enum ButtonEvent { Press, Release }
//!
//! #[async_object_with_events_decl(pub Button, pub WButton)]
//! struct ButtonImpl {
//! }
//!
//! #[async_object_impl(Button, WButton)]
//! impl ButtonImpl {
//!     async pub fn async_press(&mut self) {
//!         self.send_event(ButtonEvent::Press).await
//!     }
//!     async pub fn press(&mut self) {
//!         let _ = self.send_event(ButtonEvent::Press)
//!     }
//!     pub fn events(&self) -> EventStream<ButtonEvent> {
//!         self.create_event_stream()
//!     }
//! }
//! ```
//!
//! Below is the code which changes background color when button is pressed
//!
//! ```compile fail
//! let pool = ThreadPool::builder().create().unwrap();
//! let button = Button::new();
//! let background = Background::new();
//!
//! pool.spawn({
//!     let events = button.events();
//!     async move {
//!         // As soon as button is destroyed stream returns None
//!         while let Some(event) = events.next().await {
//!             // event has type Event<ButtonEvent>
//!             match *event.as_ref() {
//!                 ButtonEvent::Pressed => background.set_color(Color::Red).await,
//!                 ButtonEvent::Released => background.set_color(Color::White).await,
//!             }
//!         }
//!     }
//! });
//!
//! ```
//!
//! It's important to emphasize the role of Event wrapper (note that in code above stream provides Event<ButtonEvent> instances)
//! and asyncness of send_event method. The future returned by send_event is allowed to continue only when all subscribers got their instances of
//! Event<ButtonEvent> *and* these instances are dropped. This allows to pause before sending next event until the moment when previous event is fully processed
//! by subscribers.
//! If user is not interested in result of sending the event the returned future may just be dropped as in 'press' method above. Event itself will be sent anyway.
//!
//!

extern crate proc_macro;

use core::iter::Extend;
use quote::quote;
use std::{iter::once, mem::swap};
use syn::{
    self,
    parse::Parse,
    parse_macro_input,
    punctuated::Punctuated,
    token::{Async, Comma, Gt, Lt, Paren, RArrow},
    AngleBracketedGenericArguments, DeriveInput, FnArg, GenericArgument, Ident, ImplItem, ItemImpl,
    Path, PathArguments, PathSegment, ReturnType, Signature, Token, Type, TypePath, TypeTuple,
};

fn append(
    item: proc_macro::TokenStream,
    quote: proc_macro2::TokenStream,
) -> proc_macro::TokenStream {
    let mut result = proc_macro2::TokenStream::from(item);
    result.extend(once(quote));
    result.into()
}

struct WrapperDecl {
    vis: syn::Visibility,
    ident: syn::Ident,
}

impl Parse for WrapperDecl {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let look = input.lookahead1();
        let vis = if look.peek(Token![pub]) {
            input.parse()?
        } else {
            syn::Visibility::Inherited
        };
        let ident = input.parse()?;
        Ok(Self { vis, ident })
    }
}

struct AsyncObjectDeclAttr {
    carc: WrapperDecl,
    wcarc: WrapperDecl,
}

impl Parse for AsyncObjectDeclAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let carc = input.parse()?;
        let _: Comma = input.parse()?;
        let wcarc = input.parse()?;
        Ok(Self { carc, wcarc })
    }
}

#[proc_macro_attribute]
pub fn async_object_decl(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr: AsyncObjectDeclAttr = parse_macro_input!(attr);
    let derive_input = item.clone();
    let derive_input: DeriveInput = parse_macro_input!(derive_input);
    let carc_vis = attr.carc.vis;
    let carc_ident = attr.carc.ident;
    let wcarc_vis = attr.wcarc.vis;
    let wcarc_ident = attr.wcarc.ident;
    let object_ident = derive_input.ident;
    let object_vis = derive_input.vis;
    let quote = quote! {
        #carc_vis struct #carc_ident {
           carc: async_object::CArc<#object_ident>
        }
        impl #carc_ident {
            #object_vis fn create(object: #object_ident) -> Self {
                Self {
                   carc: async_object::CArc::new(object)
                }
            }
            #object_vis fn create_cyclic<F>(data_fn: F) -> Self
                where
                F: FnOnce(#wcarc_ident) -> #object_ident,
            {
                let carc = async_object::CArc::new_cyclic(|wcarc| {
                    let wcarc = #wcarc_ident {
                        wcarc
                    };
                    data_fn(wcarc)
                });
                Self {
                   carc
                }
            }
            #carc_vis fn id(&self) -> usize {
                self.carc.id()
            }
            #wcarc_vis fn downgrade(&self) -> #wcarc_ident {
                #wcarc_ident {
                   wcarc: self.carc.downgrade()
                }
            }
        }
        impl Clone for #carc_ident {
            fn clone(&self) -> Self {
                Self {
                    carc: self.carc.clone()
                }
            }
        }
        impl PartialEq for #carc_ident {
            fn eq(&self, other: &Self) -> bool {
                self.carc == other.carc
            }
        }
        #wcarc_vis struct #wcarc_ident {
            wcarc: async_object::WCArc<#object_ident>
        }
        impl #wcarc_ident {
            #wcarc_vis fn id(&self) -> Option<usize> {
                self.wcarc.id()
            }
            #carc_vis fn upgrade(&self) -> Option<#carc_ident> {
                if let Some(carc) = self.wcarc.upgrade() {
                    Some(#carc_ident {
                        carc
                    })
                } else {
                    None
                }
            }
        }
        impl Clone for #wcarc_ident {
            fn clone(&self) -> Self {
                Self {
                    wcarc: self.wcarc.clone()
                }
            }
        }
        impl Default for #wcarc_ident {
            fn default() -> Self {
                Self {
                    wcarc: async_object::WCArc::<#object_ident>::default()
                }
            }
        }
    };
    append(item, quote)
}

#[proc_macro_attribute]
pub fn async_object_with_events_decl(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr: AsyncObjectDeclAttr = parse_macro_input!(attr);
    let derive_input = item.clone();
    let derive_input: DeriveInput = parse_macro_input!(derive_input);
    let carc_vis = attr.carc.vis;
    let carc_ident = attr.carc.ident;
    let wcarc_vis = attr.wcarc.vis;
    let wcarc_ident = attr.wcarc.ident;
    let object_ident = derive_input.ident;
    let object_vis = derive_input.vis;
    let quote = quote! {
        #carc_vis struct #carc_ident {
            carc: async_object::CArc<#object_ident>,
            earc: async_object::EArc
        }
        impl #carc_ident {
            #object_vis fn create(object: #object_ident) -> Self {
                Self {
                    carc: async_object::CArc::new(object),
                    earc: async_object::EArc::new()
                }
            }
            #object_vis fn create_cyclic<F>(data_fn: F) -> Self
                where
                F: FnOnce(#wcarc_ident) -> #object_ident,
            {
                let earc = async_object::EArc::new();
                let carc = async_object::CArc::new_cyclic(|wcarc| {
                    let wcarc = #wcarc_ident {
                        wcarc,
                        wearc: earc.downgrade()
                    };
                    data_fn(wcarc)
                });
                Self {
                   carc,
                   earc
                }
            }
            #carc_vis fn id(&self) -> usize {
                self.carc.id()
            }
            #wcarc_vis fn downgrade(&self) -> #wcarc_ident {
                #wcarc_ident {
                    wcarc: self.carc.downgrade(),
                    wearc: self.earc.downgrade()
                }
            }
            async fn send_event<EVT: Send + Sync + 'static>(&self, event: EVT) {
                self.earc.send_event(event).await
            }
            fn create_event_stream<EVT: Send + Sync + 'static>(&self) -> async_object::EventStream<EVT> {
                async_object::EventStream::new(&self.earc)
            }
        }
        impl Clone for #carc_ident {
            fn clone(&self) -> Self {
                #carc_ident {
                    carc: self.carc.clone(),
                    earc: self.earc.clone()
                }
            }
        }
        impl PartialEq for #carc_ident {
            fn eq(&self, other: &Self) -> bool {
                self.carc == other.carc
            }
        }
        #wcarc_vis struct #wcarc_ident {
            wcarc: async_object::WCArc<#object_ident>,
            wearc: async_object::WEArc
        }
        impl #wcarc_ident {
            #carc_vis fn upgrade(&self) -> Option<#carc_ident> {
                if let (Some(carc), Some(earc)) = (self.wcarc.upgrade(), self.wearc.upgrade()) {
                    Some(#carc_ident {
                        carc, earc
                    })
                } else {
                    None
                }
            }
            #wcarc_vis fn id(&self) -> Option<usize> {
                self.wcarc.id()
            }
        }
        impl Clone for #wcarc_ident {
            fn clone(&self) -> Self {
                #wcarc_ident {
                    wcarc: self.wcarc.clone(),
                    wearc: self.wearc.clone()
                }
            }
        }
        impl Default for #wcarc_ident {
            fn default() -> Self {
                Self {
                    wcarc: async_object::WCArc::<#object_ident>::default(),
                    wearc: async_object::WEArc::default(),
                }
            }
        }

    };
    append(item, quote)
}

struct AsyncObjectImplAttr {
    carc: Ident,
    wcarc: Ident,
}

impl Parse for AsyncObjectImplAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let carc = input.parse()?;
        let _: Comma = input.parse()?;
        let wcarc = input.parse()?;
        Ok(Self { carc, wcarc })
    }
}

fn wrap_to_option(mut param: Box<Type>) -> (Box<Type>, bool) {
    let result_arg = if let Type::Path(ref mut param) = *param {
        if let Some(param) = param.path.segments.last_mut() {
            if param.ident == "Result" {
                if let PathArguments::AngleBracketed(ref mut args) = param.arguments {
                    if let Some(result_arg) = args.args.first_mut() {
                        Some(result_arg)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    let option_ident = Ident::new("Option", proc_macro2::Span::call_site());
    let mut segments = Punctuated::new();
    if let Some(result_arg) = result_arg {
        // *param == Result<RET_TYPE, ...>
        // goal is to wrap RET_TYPE to Option, not whole Result:
        // *param == Result<Option<RET_TYPE>. ...>
        let option_arguments = PathArguments::AngleBracketed(AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Lt::default(),
            args: Punctuated::new(),
            gt_token: Gt::default(),
        });
        segments.push_value(PathSegment {
            ident: option_ident,
            arguments: option_arguments,
        });
        let mut arg = GenericArgument::Type(Type::Path(TypePath {
            qself: None,
            path: Path {
                leading_colon: None,
                segments,
            },
        }));
        // Before swap:
        // *param == Result<&mut result_arg = RET_TYPE, ...>
        // arg == Option<>
        swap(result_arg, &mut arg);
        // After swap:
        // *param == Result<&mut result_arg = Option<>,...>
        // arg == RET_TYPE

        // Then insert RET_TYPE to Option<>:
        // after this
        // *param == Result<Option<RET_TYPE>, ...>
        if let GenericArgument::Type(ref mut v) = result_arg {
            if let Type::Path(ref mut v) = v {
                if let Some(v) = v.path.segments.first_mut() {
                    if let PathArguments::AngleBracketed(ref mut v) = v.arguments {
                        v.args.push(arg)
                    }
                }
            }
        }

        // result_args.push_value(option);
        (Box::new(*param), true)
    } else {
        // Just wrap return value to Option
        let option_generic_argument = GenericArgument::Type(*param);
        let mut args = Punctuated::new();
        args.push_value(option_generic_argument);
        let option_arguments = PathArguments::AngleBracketed(AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Lt::default(),
            args,
            gt_token: Gt::default(),
        });
        segments.push_value(PathSegment {
            ident: option_ident,
            arguments: option_arguments,
        });
        let option = Type::Path(TypePath {
            qself: None,
            path: Path {
                leading_colon: None,
                segments,
            },
        });
        (Box::new(option), false)
    }
}

#[proc_macro_attribute]
pub fn async_object_impl(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr: AsyncObjectImplAttr = parse_macro_input!(attr);
    let carc_ident: Ident = attr.carc;
    let wcarc_ident: Ident = attr.wcarc;
    let item_impl = item.clone();
    let item_impl: ItemImpl = parse_macro_input!(item_impl);
    let mut carc_methods = Vec::new();
    let mut wcarc_methods = Vec::new();
    for item in item_impl.items {
        if let ImplItem::Method(method) = item {
            let vis = method.vis;
            let signature = method.sig;
            if signature.asyncness.is_none() {
                if let Some(FnArg::Receiver(first_param)) = signature.inputs.first() {
                    let method_name = &signature.ident;
                    let mut s = "async_".to_string();
                    s += method_name.to_string().as_str();
                    let async_method_name = Ident::new(s.as_str(), proc_macro2::Span::call_site());
                    let param_names = signature
                        .inputs
                        .iter()
                        .filter_map(|v| {
                            if let FnArg::Typed(arg) = v {
                                Some(&arg.pat)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    let async_signature = Signature {
                        asyncness: Some(Async::default()),
                        ident: async_method_name.clone(),
                        ..signature.clone()
                    };
                    let methods = if let Some(_) = first_param.mutability {
                        quote! {
                            #vis #signature {
                                self.carc.call_mut(|v| v.#method_name(#(#param_names),*) )
                            }
                            #vis #async_signature {
                                self.carc.async_call_mut(|v| v.#method_name(#(#param_names),*) ).await
                            }
                        }
                    } else {
                        quote! {
                            #vis #signature {
                                self.carc.call(|v| v.#method_name(#(#param_names),*) )
                            }
                            #vis #async_signature {
                                self.carc.async_call(|v| v.#method_name(#(#param_names),*) ).await
                            }
                        }
                    };
                    carc_methods.push(methods);

                    let (result, transpose) = wrap_to_option(
                        if let ReturnType::Type(_, ref outtype) = signature.output {
                            outtype.clone()
                        } else {
                            Box::new(Type::Tuple(TypeTuple {
                                paren_token: Paren::default(),
                                elems: Punctuated::default(),
                            }))
                        },
                    );
                    let transpose_call = if transpose {
                        quote! { . transpose() }
                    } else {
                        quote! {}
                    };
                    let none_result = if transpose {
                        quote! { Ok(None) }
                    } else {
                        quote! { None }
                    };

                    let output = ReturnType::Type(RArrow::default(), result);

                    let signature = Signature {
                        output: output.clone(),
                        ..signature.clone()
                    };
                    let async_signature = Signature {
                        asyncness: Some(Async::default()),
                        ident: async_method_name,
                        output,
                        ..signature.clone()
                    };
                    let methods = if let Some(_) = first_param.mutability {
                        quote! {
                            #vis #signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.call_mut(|v| v.#method_name(#(#param_names),*) )) #transpose_call
                                } else {
                                    #none_result
                                }
                            }
                            #vis #async_signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.async_call_mut(|v| v.#method_name(#(#param_names),*) ).await) #transpose_call
                                } else {
                                    #none_result
                                }
                            }
                        }
                    } else {
                        quote! {
                            #vis #signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.call(|v| v.#method_name(#(#param_names),*) )) #transpose_call
                                } else {
                                    #none_result
                                }
                            }
                            #vis #async_signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.async_call(|v| v.#method_name(#(#param_names),*) ).await) #transpose_call
                                } else {
                                    #none_result
                                }
                            }
                        }
                    };
                    wcarc_methods.push(methods);
                }
            }
        }
    }
    let quote = quote! {
        impl #carc_ident {
            #(#carc_methods)*
        }
        impl #wcarc_ident {
            #(#wcarc_methods)*
        }
    };
    append(item, quote)
}
