use postgres_protocol::types::{array_to_sql, ArrayDimension};
use postgres_types::{private::BytesMut, IsNull, Kind, ToSql, Type};
use std::{
    error::Error,
    fmt::{Debug, Formatter},
};

use crate::utils::escape_domain;

pub struct Domain<T: ToSql>(pub T);

impl<T: ToSql + Debug> Debug for Domain<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DomainWrapper").field(&self.0).finish()
    }
}

impl<T: ToSql> ToSql for Domain<T> {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>>
    where
        Self: Sized,
    {
        postgres_types::ToSql::to_sql(&self.0, escape_domain(ty), out)
    }

    fn accepts(ty: &Type) -> bool
    where
        Self: Sized,
    {
        return T::accepts(escape_domain(ty));
    }

    fn to_sql_checked(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        postgres_types::__to_sql_checked(self, ty, out)
    }
}

pub struct DomainArray<'a, T: ToSql>(pub &'a [T]);

impl<'a, T: ToSql> Debug for DomainArray<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ArrayDomain").field(&self.0).finish()
    }
}

impl<'a, T: ToSql + 'a> ToSql for DomainArray<'a, T> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let member_type = match *ty.kind() {
            Kind::Array(ref member) => escape_domain(member),
            _ => panic!("expected array type got {}", ty),
        };

        let dimension = ArrayDimension {
            len: downcast(self.0.len())?,
            lower_bound: 1,
        };

        array_to_sql(
            Some(dimension),
            member_type.oid(),
            self.0.iter(),
            |e, w| match Domain(e).to_sql(member_type, w)? {
                IsNull::No => Ok(postgres_protocol::IsNull::No),
                IsNull::Yes => Ok(postgres_protocol::IsNull::Yes),
            },
            w,
        )?;
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        match *ty.kind() {
            Kind::Array(ref member) => T::accepts(escape_domain(member)),
            _ => false,
        }
    }

    fn to_sql_checked(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        postgres_types::__to_sql_checked(self, ty, out)
    }
}

fn downcast(len: usize) -> Result<i32, Box<dyn Error + Sync + Send>> {
    if len > i32::max_value() as usize {
        Err("value too large to transmit".into())
    } else {
        Ok(len as i32)
    }
}
