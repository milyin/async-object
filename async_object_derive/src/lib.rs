extern crate proc_macro;

use core::iter::Extend;
use quote::quote;
use std::iter::once;
use syn::{self, parse_macro_input};

enum ObjectMacroTitle {
    AsyncObject(String),
    AsyncObjectEvents(String),
}

fn append(
    item: proc_macro::TokenStream,
    quote: proc_macro2::TokenStream,
) -> proc_macro::TokenStream {
    let mut result = proc_macro2::TokenStream::from(item);
    result.extend(once(quote));
    result.into()
}

#[proc_macro_attribute]
pub fn async_object(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let ident: syn::Ident = parse_macro_input!(attr);
    append(
        item,
        quote! {
            struct #ident;
        },
    )
}
