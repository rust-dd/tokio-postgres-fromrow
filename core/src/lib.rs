use darling::{Error, FromDeriveInput, FromField, ast};
use proc_macro::TokenStream;
use quote::ToTokens;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(FromRow)]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);

    match try_derive_from_row(&derive_input) {
        Ok(result) => result,
        Err(err) => err.write_errors().into(),
    }
}

fn try_derive_from_row(input: &DeriveInput) -> Result<TokenStream, Error> {
    let from_row_derive = DeriveFromRow::from_derive_input(input)?;
    Ok(from_row_derive.generate()?)
}

#[derive(Debug, FromDeriveInput)]
#[darling(
    // attributes(from_row),
    forward_attrs(allow, doc, cfg),
    supports(struct_named)
)]
struct DeriveFromRow {
    ident: syn::Ident,
    vis: syn::Visibility,
    generics: syn::Generics,
    data: ast::Data<(), FromRowField>,
    attrs: Vec<syn::Attribute>,
}

impl DeriveFromRow {
    fn generate(&self) -> syn::Result<TokenStream> {
        // get fields
        let fields = match &self.data {
            ast::Data::Struct(fields) => fields.fields.as_slice(),
            _ => unimplemented!(),
        };

        // validate fields
        for f in fields {
            f.validate()?;
        }

        Ok(TokenStream::new())
    }
}

#[derive(Debug, FromField)]
#[darling(attributes(from_row), forward_attrs(allow, doc, cfg))]
struct FromRowField {
    ident: Option<syn::Ident>,
    vis: syn::Visibility,
    ty: syn::Type,
    from: Option<String>,
    try_from: Option<String>,
    raname: Option<String>,
}

impl FromRowField {
    /// Validate all rules on field level
    fn validate(&self) -> syn::Result<()> {
        if self.from.is_some() && self.try_from.is_some() {
            return Err(Error::custom(r#"cannot specify both `from` and `try_from`"#).into());
        }

        Ok(())
    }

    /// Get the target field type
    fn target_field_ty(&self) -> syn::Result<proc_macro2::TokenStream> {
        if let Some(from) = &self.from {
            return Ok(from.parse()?);
        }

        if let Some(try_from) = &self.try_from {
            return Ok(try_from.parse()?);
        }

        Ok(self.ty.to_token_stream())
    }

    /// Handle renaming of fields
    fn target_field_name(&self) -> String {
        self.raname
            .as_ref()
            .map(Clone::clone)
            .unwrap_or(self.ident.as_ref().unwrap().to_string())
    }
}
