use darling::{Error, FromDeriveInput, FromField, ast};
use proc_macro::TokenStream;
use quote::{ToTokens, quote};
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

        let ident = &self.ident;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        let original_predicates = where_clause.clone().map(|w| &w.predicates).into_iter();
        let mut predicates = Vec::new();

        for f in fields {
            f.add_predicates(&mut predicates)?;
        }

        let from_row_fields = fields
            .iter()
            .map(|f| f.generate_from_row())
            .collect::<syn::Result<Vec<_>>>()?;
        let try_from_row_fields = fields
            .iter()
            .map(|f| f.generate_try_from_row())
            .collect::<syn::Result<Vec<_>>>()?;

        let quote = quote! {
            impl #impl_generics tokio_postgres_fromrow::FromRow for #ident #ty_generics where #(#original_predicates),* #(#predicates),* {
                fn from_row(row: &tokio_postgres_fromrow::tokio_postgres::Row) -> Self {
                    Self {
                        #(#from_row_fields),*
                    }
                }

                fn try_from_row(row: &tokio_postgres_fromrow::tokio_postgres::Row) -> Result<Self, tokio_postgres_fromrow::tokio_postgres::Error> {
                    Ok(Self {
                        #(#try_from_row_fields),*
                    })
                }
            }
        };

        Ok(quote.into())
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
    attrs: Vec<syn::Attribute>,
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

    /// Add predicates
    fn add_predicates(&self, predicates: &mut Vec<proc_macro2::TokenStream>) -> syn::Result<()> {
        let ty = &self.ty;
        let target_ty = self.target_field_ty()?;

        predicates.push(
            quote!(#target_ty: for<'__from_row> tokio_postgres_fromrow::tokio_postgres::types::FromSql<'__from_row>),
        );

        if self.from.is_some() {
            predicates.push(quote!(#ty: std::convert::From<#target_ty>));
        }

        if self.try_from.is_some() {
            let try_from = quote!(std::convert::TryFrom<#target_ty>);

            predicates.push(quote!(#ty: #try_from));
            predicates.push(quote!(tokio_postgres_fromrow::Error: std::convert::From<<#ty as #try_from>::Error>));
            predicates.push(quote!(<#ty as #try_from>::Error: std::fmt::Debug));
        }

        Ok(())
    }

    fn generate_from_row(&self) -> syn::Result<proc_macro2::TokenStream> {
        let ident = self.ident.as_ref().unwrap();
        let vis = &self.vis;
        let col_name = self.target_field_name();
        let ty = &self.ty;
        let target_ty = self.target_field_ty()?;
        let attrs = &self.attrs;

        let field_expr = quote!(tokio_postgres_fromrow::tokio_postgres::Row::get::<&str, #target_ty>(row, #col_name));

        // add from, try_from

        Ok(quote! {
            #(#attrs)*
            #vis #ident: #field_expr,
        })
    }

    fn generate_try_from_row(&self) -> syn::Result<proc_macro2::TokenStream> {
        let ident = self.ident.as_ref().unwrap();
        let vis = &self.vis;
        let col_name = self.target_field_name();
        let ty = &self.ty;
        let target_ty = self.target_field_ty()?;
        let attrs = &self.attrs;

        let field_expr = quote!(tokio_postgres_fromrow::tokio_postgres::Row::get::<&str, #target_ty>(row, #col_name));

        Ok(quote! {
            #(#attrs)*
            #vis #ident: #field_expr,
        })
    }
}
