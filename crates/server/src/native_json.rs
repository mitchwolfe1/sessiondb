use serde::Serialize;
use sqlx::{
    Encode, Postgres, Type,
    encode::IsNull,
    error::BoxDynError,
    postgres::{PgArgumentBuffer, PgTypeInfo},
};

/// Bind opaque native payloads as PostgreSQL JSON, not SQLx's default JSONB.
/// JSONB rejects valid escaped NUL characters found in native tool output.
/// Serialize directly into the argument buffer without an intermediate string.
pub(crate) struct NativeJson<T>(pub T);

impl<T> Type<Postgres> for NativeJson<T> {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("json")
    }
}

impl<T: Serialize> Encode<'_, Postgres> for NativeJson<T> {
    fn encode_by_ref(&self, buffer: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        serde_json::to_writer(&mut **buffer, &self.0)?;
        Ok(IsNull::No)
    }
}
