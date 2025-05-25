use darling::{Error, FromDeriveInput, FromField, ast};
use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    DeriveInput, Field, Fields, ItemStruct, Path, Type, parse_macro_input, parse_quote,
    punctuated::Punctuated, token::Comma,
};

#[proc_macro_attribute]
pub fn from_row(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr with Punctuated::<Path, Comma>::parse_terminated);
    let has_transform = args.iter().any(|p| p.is_ident("transform_option"));

    let mut item_struct = parse_macro_input!(item as ItemStruct);
    if has_transform {
        wrap_fields_into_option(&mut item_struct.fields);
    }

    TokenStream::from(quote!(#item_struct))
}

fn wrap_fields_into_option(fields: &mut Fields) {
    let make_option = |ty: &Type| -> Type { parse_quote!(::core::option::Option<#ty>) };

    match fields {
        Fields::Named(named) => {
            for Field { ty, .. } in &mut named.named {
                if !is_option_type(&ty) {
                    *ty = make_option(&ty.clone());
                }
            }
        }
        _ => {}
    }
}

fn is_option_type(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) if tp.qself.is_none() => tp
            .path
            .segments
            .last()
            .map(|s| s.ident == "Option")
            .unwrap_or(false),
        _ => false,
    }
}

#[proc_macro_derive(FromRow)]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);
    match DeriveFromRow::from_derive_input(&derive_input)
        .and_then(|d| d.generate().map_err(|e| darling::Error::from(e)))
    {
        Ok(tokens) => tokens,
        Err(err) => err.write_errors().into(),
    }
}

#[derive(Debug, FromDeriveInput)]
#[darling(forward_attrs(allow, doc, cfg), supports(struct_named))]
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
        let mut predicates = Vec::new();
        if let Some(w) = where_clause {
            predicates.extend(w.predicates.iter().map(|p| quote!(#p)));
        }
        for f in fields {
            f.add_predicates(&mut predicates)?;
        }

        let from_row_fields = fields
            .iter()
            .map(|f| f.generate_field_expr())
            .collect::<syn::Result<Vec<_>>>()?;

        let quote = quote! {
            impl #impl_generics tokio_postgres_fromrow::FromRow for #ident #ty_generics
                where #(#predicates),*
            {
                fn from_row(row: &tokio_postgres_fromrow::tokio_postgres::Row) -> Self {
                    Self { #(#from_row_fields),* }
                }

                fn try_from_row(row: &tokio_postgres_fromrow::tokio_postgres::Row) -> Result<Self, tokio_postgres_fromrow::tokio_postgres::Error> {
                    Ok(Self::from_row(row))
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

    /// transformation
    from: Option<String>,
    try_from: Option<String>,

    /// rename attr
    rename: Option<String>,
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
    fn row_ty(&self) -> syn::Result<proc_macro2::TokenStream> {
        if let Some(ref s) = self.from {
            return Ok(s.parse()?);
        }
        if let Some(ref s) = self.try_from {
            return Ok(s.parse()?);
        }
        Ok(self.ty.to_token_stream())
    }

    /// Handle renaming of fields
    fn column_name(&self) -> String {
        self.rename
            .clone()
            .unwrap_or_else(|| self.ident.as_ref().unwrap().to_string())
    }

    /// Add predicates
    fn add_predicates(&self, preds: &mut Vec<proc_macro2::TokenStream>) -> syn::Result<()> {
        let target_ty = self.row_ty()?;
        let ty = &self.ty;

        preds.push(quote!(
            #target_ty: for<'__fr> tokio_postgres_fromrow::tokio_postgres::types::FromSql<'__fr>
        ));

        if self.from.is_some() {
            preds.push(quote!(#ty: ::core::convert::From<#target_ty>));
        }
        if self.try_from.is_some() {
            let try_from = quote!(::core::convert::TryFrom<#target_ty>);
            preds.push(quote!(#ty: #try_from));
            preds.push(quote!(
                tokio_postgres_fromrow::Error:
                    ::core::convert::From<<#ty as #try_from>::Error>
            ));
            preds.push(quote!(<#ty as #try_from>::Error: ::core::fmt::Debug));
        }
        if !is_option_type(ty) {
            preds.push(quote!(#ty: ::core::default::Default));
        }
        Ok(())
    }

    fn generate_field_expr(&self) -> syn::Result<proc_macro2::TokenStream> {
        let ident = self.ident.as_ref().unwrap();
        let col = self.column_name();
        let target = self.row_ty()?;
        let is_opt = is_option_type(&self.ty);
        let attrs = &self.attrs;

        let expr = if is_opt {
            quote!({
                if row.columns().iter().any(|c| c.name() == #col) {
                    row.try_get::<&str, #target>(#col).ok().flatten()
                } else {
                    None
                }
            })
        } else {
            quote!({
                if row.columns().iter().any(|c| c.name() == #col) {
                    row.try_get::<&str, #target>(#col).unwrap_or_default()
                } else {
                    ::core::default::Default::default()
                }
            })
        };

        Ok(quote! {
            #(#attrs)*
            #ident: #expr,
        })
    }
}
