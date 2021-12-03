extern crate proc_macro;

use core::iter::Extend;
use quote::quote;
use std::iter::once;
use syn::{self, parse::Parse, parse_macro_input, DeriveInput, Ident, Token, Visibility};

fn append(
    item: proc_macro::TokenStream,
    quote: proc_macro2::TokenStream,
) -> proc_macro::TokenStream {
    let mut result = proc_macro2::TokenStream::from(item);
    result.extend(once(quote));
    result.into()
}

struct AsyncObjectDecl {
    visibility: syn::Visibility,
    ident: syn::Ident,
}

impl Parse for AsyncObjectDecl {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let look = input.lookahead1();
        let visibility = if look.peek(Token![pub]) {
            input.parse()?
        } else {
            syn::Visibility::Inherited
        };
        let ident = input.parse()?;
        Ok(AsyncObjectDecl { visibility, ident })
    }
}

#[proc_macro_attribute]
pub fn async_object(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let decl: AsyncObjectDecl = parse_macro_input!(attr);
    let item1 = item.clone();
    let derive_input: DeriveInput = parse_macro_input!(item1);
    let async_object_visibility = decl.visibility;
    let async_object_ident = decl.ident;
    let wrapped_object_ident = derive_input.ident;
    let wrapped_object_visibility = derive_input.vis;
    let quote = quote! {
        #async_object_visibility struct #async_object_ident {
            carc: async_object::CArc<#wrapped_object_ident>
        }
        impl #async_object_ident {
            #wrapped_object_visibility fn create(object: #wrapped_object_ident) -> Self {
                Self {
                    carc: async_object::CArc::new(object)
                }
            }
        }
    };
    append(item, quote)
}
