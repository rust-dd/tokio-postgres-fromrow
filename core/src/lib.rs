use darling::{Error, ast};
use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(FromRow)]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);

    match try_derive_from_row(&derive_input) {
        Ok(result) => result,
        Err(err) => err.write_errors().into(),
    }
}

fn try_derive_from_row(input: &DeriveInput) -> Result<TokenStream, Error> {}

struct DeriveFromRow {
    ident: syn::Ident,
    generics: syn::Generics,
    data: ast::Data<(), ()>,
}
