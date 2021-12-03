extern crate proc_macro;

use core::iter::Extend;
use quote::quote;
use std::iter::once;
use syn::{self, parse::Parse, parse_macro_input, Ident, Token, Visibility};

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
    let visibility = decl.visibility;
    let ident = decl.ident;
    let quote = quote! {
        #visibility struct #ident;
    };
    append(item, quote)
}
