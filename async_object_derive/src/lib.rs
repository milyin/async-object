extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn;

enum ObjectMacroTitle {
    AsyncObject(String),
    AsyncObjectEvents(String),
}

#[proc_macro_attribute]
pub fn async_object_macro_derive(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = syn::parse(attr).unwrap();
    let item = syn::parse(item).unwrap();

    let title = parse_async_object_macro_title(&attr);

    impl_async_object_macro(title, &item)
}

fn parse_async_object_macro_title(ast: &syn::DeriveInput) -> ObjectMacroTitle {
    ObjectMacroTitle::AsyncObject("".to_string())
}

fn impl_async_object_macro(title: ObjectMacroTitle, ast: &syn::DeriveInput) -> TokenStream {
    let gen = quote! {};
    gen.into()
}
