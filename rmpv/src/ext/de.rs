use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};
use std::iter::ExactSizeIterator;
use std::marker::PhantomData;

use serde::de::{self, DeserializeSeed, SeqAccess, Unexpected, Visitor};
use serde::forward_to_deserialize_any;
use serde::{self, Deserialize, Deserializer};

use crate::{IntPriv, Integer, Utf8String, Utf8StringRef, Value, ValueRef};

use super::{Error, ValueExt};
use crate::MSGPACK_EXT_STRUCT_NAME;

#[inline]
pub fn from_value<T>(val: Value) -> Result<T, Error>
where
    T: for<'de> Deserialize<'de>,
{
    deserialize_from(val)
}

#[inline]
pub fn deserialize_from<'de, T, D>(val: D) -> Result<T, Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de, Error = Error>,
{
    Deserialize::deserialize(val)
}

impl de::Error for Error {
    #[cold]
    fn custom<T: Display>(msg: T) -> Self {
        Self::Syntax(format!("{msg}"))
    }
}

macro_rules! impl_deserialize {
    ($value_type: ty { $($extra_visitors: tt)* } ) => {
        impl<'de> Deserialize<'de> for $value_type {
            #[inline]
            fn deserialize<D>(de: D) -> Result<Self, D::Error>
                where D: de::Deserializer<'de>
            {
                struct ValueVisitor;

                impl<'de> serde::de::Visitor<'de> for ValueVisitor {
                    type Value = $value_type;

                    #[cold]
                    fn expecting(&self, fmt: &mut Formatter<'_>) -> Result<(), fmt::Error> {
                        "any valid MessagePack value".fmt(fmt)
                    }

                    #[inline]
                    fn visit_some<D>(self, de: D) -> Result<Self::Value, D::Error>
                        where D: de::Deserializer<'de>
                    {
                        Deserialize::deserialize(de)
                    }

                    #[inline]
                    fn visit_none<E>(self) -> Result<Self::Value, E> {
                        Ok(Self::Value::Nil)
                    }

                    #[inline]
                    fn visit_unit<E>(self) -> Result<Self::Value, E> {
                        Ok(Self::Value::Nil)
                    }

                    #[inline]
                    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                        Ok(Self::Value::Boolean(value))
                    }

                    #[inline]
                    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                        Ok(Self::Value::from(value))
                    }

                    #[inline]
                    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                        Ok(Self::Value::from(value))
                    }

                    #[inline]
                    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E> {
                        Ok(Self::Value::F32(value))
                    }

                    #[inline]
                    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                        Ok(Self::Value::F64(value))
                    }

                    #[inline]
                    fn visit_seq<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
                        where V: SeqAccess<'de>
                    {
                        let mut vec = Vec::new();
                        while let Some(elem) = visitor.next_element()? {
                            vec.push(elem);
                        }
                        Ok(Self::Value::Array(vec))
                    }

                    #[inline]
                    fn visit_map<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
                        where V: de::MapAccess<'de>
                    {
                        let mut pairs = vec![];

                        while let Some(key) = visitor.next_key()? {
                            let val = visitor.next_value()?;
                            pairs.push((key, val));
                        }

                        Ok(Self::Value::Map(pairs))
                    }

                    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
                        where D: Deserializer<'de>,
                    {

                        deserializer.deserialize_tuple(2, ExtValueVisitor(PhantomData))
                    }

                    $($extra_visitors)*
                }

                de.deserialize_any(ValueVisitor)
            }
        }
    }
}

struct ExtValueVisitor<ValueType>(PhantomData<ValueType>);

impl<'de, ValueType> serde::de::Visitor<'de> for ExtValueVisitor<ValueType>
where
    ExtValueVisitor<ValueType>: ToExtFromBytes<'de, Value = ValueType>,
{
    type Value = ValueType;

    #[cold]
    fn expecting(&self, fmt: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        "a valid MessagePack Ext".fmt(fmt)
    }

    #[inline]
    fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
    where
        V: SeqAccess<'de>,
    {
        let tag = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;

        let bytes = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;

        Ok(self.to_ext_from_bytes(tag, bytes))
    }
}

trait ToExtFromBytes<'de> {
    type Value;
    type Bytes: Deserialize<'de>;
    fn to_ext_from_bytes(&self, tag: i8, bytes: Self::Bytes) -> Self::Value;
}

impl ToExtFromBytes<'_> for ExtValueVisitor<Value> {
    type Value = Value;
    type Bytes = serde_bytes::ByteBuf;
    #[inline]
    fn to_ext_from_bytes(&self, tag: i8, bytes: Self::Bytes) -> Self::Value {
        Value::Ext(tag, bytes.to_vec())
    }
}

impl<'de> ToExtFromBytes<'de> for ExtValueVisitor<ValueRef<'de>> {
    type Value = ValueRef<'de>;
    type Bytes = &'de [u8];
    #[inline]
    fn to_ext_from_bytes(&self, tag: i8, bytes: Self::Bytes) -> Self::Value {
        ValueRef::Ext(tag, bytes)
    }
}

impl_deserialize!(Value {
    #[inline]
    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Self::Value::String(Utf8String::from(value)))
    }

    #[inline]
    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where E: de::Error
    {
        self.visit_string(String::from(value))
    }

    #[inline]
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where E: de::Error
    {
        Ok(Self::Value::Binary(v.to_owned()))
    }

    #[inline]
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
        where E: de::Error
    {
        Ok(Self::Value::Binary(v))
    }
});

impl_deserialize!(ValueRef<'de> {
    #[inline]
    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
        where E: de::Error
    {
        Ok(ValueRef::String(Utf8StringRef::from(value)))
    }

    #[inline]
    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
        where E: de::Error
    {
        Ok(ValueRef::Binary(v))
    }
});

struct VariantDeserializer<U> {
    value: Option<U>,
}

macro_rules! impl_deserializer {
    ($value_type: ident, $self_type: ty, $iter: ident, $visit_string: ident, $visit_bytes: ident, $new_ext: ident $(,$ref: tt, $deref:tt )?) => {
        impl<'de> Deserializer<'de> for $self_type {
            type Error = Error;

            fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
                where V: Visitor<'de>
            {
                match $($deref)? self {
                    $value_type::Nil => visitor.visit_unit(),
                    $value_type::Boolean(v) => visitor.visit_bool(v),
                    $value_type::Integer(Integer { n }) => match n {
                        IntPriv::PosInt(v) => visitor.visit_u64(v),
                        IntPriv::NegInt(v) => visitor.visit_i64(v),
                    },
                    $value_type::F32(v) => visitor.visit_f32(v),
                    $value_type::F64(v) => visitor.visit_f64(v),
                    $value_type::String(v) => {
                        match v.s {
                            Ok(v) => visitor.$visit_string(v),
                            Err(v) => visitor.$visit_bytes(v.0),
                        }
                    }
                    $value_type::Binary(v) => {
                        visitor.$visit_bytes(v)
                    }
                    $value_type::Array($($ref)? v) => {
                        let len = v.len();
                        let mut de = SeqDeserializer::new(v.$iter());
                        let seq = visitor.visit_seq(&mut de)?;
                        if de.iter.len() == 0 {
                            Ok(seq)
                        } else {
                            Err(de::Error::invalid_length(len, &"fewer elements in array"))
                        }
                    }
                    $value_type::Map($($ref)? v) => {
                        let len = v.len();
                        let mut de = MapDeserializer::new(v.$iter());
                        let map = visitor.visit_map(&mut de)?;
                        if de.iter.len() == 0 {
                            Ok(map)
                        } else {
                            Err(de::Error::invalid_length(len, &"fewer elements in map"))
                        }
                    }
                    $value_type::Ext(tag, data) => {
                        let de = ExtDeserializer::$new_ext(tag, data);
                        visitor.visit_newtype_struct(de)
                    }
                }
            }

            #[inline]
            fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
                where V: Visitor<'de>
            {
                match self {
                    $value_type::Nil => visitor.visit_none(),
                    value => visitor.visit_some(value),
                }
            }

            #[inline]
            fn deserialize_enum<V>(self, _name: &str, _variants: &'static [&'static str], visitor: V) -> Result<V::Value, Self::Error>
                where V: Visitor<'de>
            {
                match self {
                    $value_type::Array(arr) => {
                        if !(arr.len() == 1 || arr.len() == 2) {
                            return Err(de::Error::invalid_length(arr.len(), &"array with one or two elements"));
                        }
                        let mut iter = arr.$iter();
                        let id = match iter.next() {
                            Some(id) => id,
                            None => {
                                return Err(de::Error::invalid_value(Unexpected::Seq, &"array with one or two elements"));
                            }
                        };

                        visitor.visit_enum(EnumDeserializer::new(id, iter.next()))
                    },
                    $value_type::Map(map) => {
                        if (map.len() != 1) {
                            return Err(de::Error::invalid_length(map.len(), &"map with one element"));
                        }
                        let mut iter = map.$iter();
                        let (id, value) = iter.next().unwrap();

                        visitor.visit_enum(EnumDeserializer::new(id, Some(value)))
                    }
                    str @ $value_type::String(..) => {
                        visitor.visit_enum(EnumDeserializer::new(str, None))
                    }
                    other => {
                        Err(de::Error::invalid_type(other.unexpected(), &"array, map, int or string"))
                    }
                }
            }

            #[inline]
            fn deserialize_newtype_struct<V>(self, name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
                where V: Visitor<'de>
            {
                if name == MSGPACK_EXT_STRUCT_NAME {
                    match self {
                        $value_type::Ext(tag, data) => {
                            let ext_de = ExtDeserializer::$new_ext($($deref)? tag, $($deref)? data);
                            return visitor.visit_newtype_struct(ext_de);
                        }
                        other => return Err(de::Error::invalid_type(other.unexpected(), &"expected Ext")),
                    }
                }

                visitor.visit_newtype_struct(self)
            }

            #[inline]
            fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Self::Error>
                where V: Visitor<'de>
            {
                 match self {
                    $value_type::Array(arr) => {
                        if arr.len() == 0 {
                            visitor.visit_unit()
                        } else {
                            Err(de::Error::invalid_type(Unexpected::Seq, &"empty array"))
                        }
                    }
                    other => Err(de::Error::invalid_type(other.unexpected(), &"empty array")),
                }
            }

            forward_to_deserialize_any! {
                bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string unit seq
                bytes byte_buf map tuple_struct struct
                identifier tuple ignored_any
            }
        }

        impl<'de> de::VariantAccess<'de> for VariantDeserializer<$self_type> {
            type Error = Error;

            fn unit_variant(self) -> Result<(), Error> {
                // Can accept only [u32].
                match self.value {
                    Some($value_type::Array(arr)) if arr.len() == 0 => Ok(()),
                    Some($value_type::Array(..)) => Err(de::Error::invalid_value(Unexpected::Seq, &"empty array")),
                    Some(v) => Err(de::Error::invalid_value(v.unexpected(), &"empty array")),
                    None => Ok(()),
                }
            }

            fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Error>
                where T: de::DeserializeSeed<'de>
            {
                // Can accept both [u32, T...] and [u32, [T]] cases.
                match self.value {
                    Some($value_type::Array(arr)) if arr.len() == 0 => Err(de::Error::invalid_value(Unexpected::Seq, &"array with one element")),
                    Some($value_type::Array(arr)) if arr.len() == 1 => seed.deserialize(arr.$iter().next().unwrap()),
                    Some(v) => seed.deserialize(v),
                    None => Err(de::Error::invalid_type(Unexpected::UnitVariant, &"newtype variant")),
                }
            }

            fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Error>
                where V: Visitor<'de>
            {
                // Can accept [u32, [T...]].
                match self.value {
                    Some(v @ $value_type::Array(..)) => v.deserialize_seq(visitor),
                    Some(v)=> Err(de::Error::invalid_type(v.unexpected(), &"tuple variant")),
                    None => Err(de::Error::invalid_type(
                        Unexpected::UnitVariant,
                        &"tuple variant",
                    )),
                }
            }

            fn struct_variant<V>(self, _fields: &'static [&'static str], visitor: V) -> Result<V::Value, Error>
                where V: Visitor<'de>,
            {
                match self.value {
                    Some(v @ $value_type::Array(..)) => v.deserialize_seq(visitor),
                    Some(v @ $value_type::Map(..)) => v.deserialize_map(visitor),
                    Some(v) => Err(de::Error::invalid_type(v.unexpected(), &"struct variant")),
                    None => Err(de::Error::invalid_type(
                        Unexpected::UnitVariant,
                        &"struct variant",
                    )),
                }
            }
        }
    }
}

impl_deserializer!(
    Value,
    Value,
    into_iter,
    visit_string,
    visit_byte_buf,
    new_owned
);
impl_deserializer!(
    ValueRef,
    ValueRef<'de>,
    into_iter,
    visit_borrowed_str,
    visit_borrowed_bytes,
    new_ref
);
impl_deserializer!(ValueRef,
    &'de ValueRef<'de>,
    iter,
    visit_borrowed_str,
    visit_borrowed_bytes,
    new_ref,
    ref,
    *);

struct ExtDeserializer<'de> {
    tag: Option<i8>,
    data: Option<Cow<'de, [u8]>>,
}

impl<'de> ExtDeserializer<'de> {
    const fn new_owned(tag: i8, data: Vec<u8>) -> Self {
        ExtDeserializer {
            tag: Some(tag),
            data: Some(Cow::Owned(data)),
        }
    }

    const fn new_ref(tag: i8, data: &'de [u8]) -> Self {
        ExtDeserializer {
            tag: Some(tag),
            data: Some(Cow::Borrowed(data)),
        }
    }
}

impl<'de> SeqAccess<'de> for ExtDeserializer<'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
    where
        T: DeserializeSeed<'de>,
    {
        if self.tag.is_some() || self.data.is_some() {
            return Ok(Some(seed.deserialize(self)?));
        }

        Ok(None)
    }
}

/// Deserializer for Ext (expecting sequence)
impl<'de> Deserializer<'de> for ExtDeserializer<'de> {
    type Error = Error;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(self)
    }

    forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string unit option
        seq bytes byte_buf map unit_struct newtype_struct
        struct identifier tuple enum ignored_any tuple_struct
    }
}

/// Deserializer for Ext `SeqAccess` elements
impl<'a, 'de: 'a> Deserializer<'de> for &'a mut ExtDeserializer<'de> {
    type Error = Error;

    #[inline]
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.tag.is_some() {
            let tag = self.tag.take().unwrap();
            visitor.visit_i8(tag)
        } else if self.data.is_some() {
            let data = self.data.take().unwrap();
            match data {
                Cow::Owned(data) => visitor.visit_byte_buf(data),
                Cow::Borrowed(data) => visitor.visit_borrowed_bytes(data),
            }
        } else {
            debug_assert!(false, "ext seq only has two elements");
            Err(Error::Syntax(String::new()))
        }
    }

    forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string unit option
        seq bytes byte_buf map unit_struct newtype_struct
        tuple_struct struct identifier tuple enum ignored_any
    }
}

struct SeqDeserializer<I> {
    iter: I,
}

impl<I> SeqDeserializer<I> {
    const fn new(iter: I) -> Self {
        Self { iter }
    }
}

impl<'de, I, U> SeqAccess<'de> for SeqDeserializer<I>
where
    I: Iterator<Item = U>,
    U: Deserializer<'de, Error = Error>,
{
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(val) => seed.deserialize(val).map(Some),
            None => Ok(None),
        }
    }
}

trait ValuePair {
    type Destination;

    fn to_reference(self) -> Self::Destination;
}

impl<'de> ValuePair for &'de (ValueRef<'de>, ValueRef<'de>) {
    type Destination = (&'de ValueRef<'de>, &'de ValueRef<'de>);

    fn to_reference(self) -> Self::Destination {
        (&self.0, &self.1)
    }
}

impl ValuePair for (Value, Value) {
    type Destination = (Value, Value);

    fn to_reference(self) -> Self::Destination {
        self
    }
}

impl<'de> ValuePair for (ValueRef<'de>, ValueRef<'de>) {
    type Destination = (ValueRef<'de>, ValueRef<'de>);

    fn to_reference(self) -> Self::Destination {
        self
    }
}

struct MapDeserializer<I, U> {
    val: Option<U>,
    iter: I,
}

impl<I, U> MapDeserializer<I, U> {
    const fn new(iter: I) -> Self {
        Self { val: None, iter }
    }
}

impl<'de, I, U> de::MapAccess<'de> for MapDeserializer<I, U>
where
    I: Iterator<Item: ValuePair<Destination = (U, U)>>,
    U: Deserializer<'de, Error = Error>,
{
    type Error = Error;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => {
                let (key, val) = value.to_reference();
                self.val = Some(val);
                seed.deserialize(key).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.val.take() {
            Some(val) => seed.deserialize(val),
            None => Err(de::Error::custom("value is missing")),
        }
    }
}

struct EnumDeserializer<U> {
    id: U,
    value: Option<U>,
}

impl<U> EnumDeserializer<U> {
    pub const fn new(id: U, value: Option<U>) -> Self {
        Self { id, value }
    }
}

impl<'de, U> de::EnumAccess<'de> for EnumDeserializer<U>
where
    U: ValueExt + Deserializer<'de, Error=Error>,
    VariantDeserializer<U>: de::VariantAccess<'de, Error = Error>,
{
    type Error = Error;
    type Variant = VariantDeserializer<U>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let variant = self.id;
        let visitor = VariantDeserializer { value: self.value };
        seed.deserialize(variant).map(|v| (v, visitor))
    }
}
