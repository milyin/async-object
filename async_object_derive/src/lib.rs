extern crate proc_macro;

use core::iter::Extend;
use quote::quote;
use std::iter::once;
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
            #wcarc_vis fn downgrade(&self) -> #wcarc_ident {
                #wcarc_ident {
                   wcarc: self.carc.downgrade()
                }
            }
        }
        impl Clone for #carc_ident {
            fn clone(&self) -> Self {
                #carc_ident {
                    carc: self.carc.clone()
                }
            }
        }
        #wcarc_vis struct #wcarc_ident {
            wcarc: async_object::WCArc<#object_ident>
        }
        impl #wcarc_ident {
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
                #wcarc_ident {
                    wcarc: self.wcarc.clone()
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
            #object_vis fn create(object: #object_ident, pool: impl futures::task::Spawn) -> Result<Self,futures::task::SpawnError> {
                Ok(Self {
                    carc: async_object::CArc::new(object),
                    earc: async_object::EArc::new(pool)?
                })
            }
            #wcarc_vis fn downgrade(&self) -> #wcarc_ident {
                #wcarc_ident {
                    wcarc: self.carc.downgrade(),
                    wearc: self.earc.downgrade()
                }
            }
            fn send_event<EVT: Send + Sync + 'static>(&self, event: EVT) {
                self.earc.send_event(event)
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
        }
        impl Clone for #wcarc_ident {
            fn clone(&self) -> Self {
                #wcarc_ident {
                    wcarc: self.wcarc.clone(),
                    wearc: self.wearc.clone()
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

fn option_of(param: Box<Type>) -> Box<Type> {
    let mut segments = Punctuated::new();
    let ident = Ident::new("Option", proc_macro2::Span::call_site());
    let mut args = Punctuated::new();
    let generic_argument = GenericArgument::Type(*param);
    args.push_value(generic_argument);
    let arguments = PathArguments::AngleBracketed(AngleBracketedGenericArguments {
        colon2_token: None,
        lt_token: Lt::default(),
        args: args,
        gt_token: Gt::default(),
    });
    segments.push_value(PathSegment { ident, arguments });
    Box::new(Type::Path(TypePath {
        qself: None,
        path: Path {
            leading_colon: None,
            segments,
        },
    }))
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

                    let output = ReturnType::Type(
                        RArrow::default(),
                        option_of(if let ReturnType::Type(_, ref outtype) = signature.output {
                            outtype.clone()
                        } else {
                            Box::new(Type::Tuple(TypeTuple {
                                paren_token: Paren::default(),
                                elems: Punctuated::default(),
                            }))
                        }),
                    );

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
                            #signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.call_mut(|v| v.#method_name(#(#param_names),*) ))
                                } else {
                                    None
                                }
                            }
                            #async_signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.async_call_mut(|v| v.#method_name(#(#param_names),*) ).await)
                                } else {
                                    None
                                }
                            }
                        }
                    } else {
                        quote! {
                            #signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.call(|v| v.#method_name(#(#param_names),*) ))
                                } else {
                                    None
                                }
                            }
                            #async_signature {
                                if let Some(v) = self.wcarc.upgrade() {
                                    Some(v.async_call(|v| v.#method_name(#(#param_names),*) ).await)
                                } else {
                                    None
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
