pub use tokio_postgres;
pub use tokio_postgres_fromrow_core::FromRow;

pub trait FromRow: Sized {
    fn from_row(row: &tokio_postgres::Row) -> Self;
}
