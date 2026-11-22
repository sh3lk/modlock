//! entity reads and writes the fields of live entities, sends them inputs and
//! creates them, through typed classes generated from the game's schema and
//! entity dump by modlock-entitygen.
//!
//! Each server entity class is a type that holds one entity's handle, such as
//! CCitadelPlayerPawn. Address an entity by its handle, then get or set a
//! field through the method named after it:
//!
//! ```ignore
//! use modlock::entity::{CCitadelPlayerPawn, Class};
//!
//! let hero = CCitadelPlayerPawn::of(pawn.entity);
//! let level = hero.m_nLevel().get();
//! ```
//!
//! A class has the fields and inputs of its bases too:
//!
//! ```ignore
//! if let Some(health) = hero.m_iHealth().get() {
//!     hero.m_iHealth().set(health + 100);
//! }
//! hero.input_Kill();
//! ```
//!
//! Each input has a pair named with _by after it, which takes after the
//! input's value the activator: the handle of the entity that caused the
//! input, such as a player's hero. An input that acts on one player, such as
//! LocalPlayerAddCSSClass, acts on the activator's player:
//!
//! ```ignore
//! panel.input_LocalPlayerAddCSSClass_by("winner", pawn.entity);
//! ```
//!
//! Every name is the game's, as the other Modlock libraries have it: a class
//! is CNPC_TrooperBoss, a field m_iHealth, an input input_Kill and a key
//! LaneNum. A name found in the game's schema, or in an example for another
//! language, is the name here.
//!
//! Each designer name is a function, whose create spawns the entity and
//! whose create_with also sets its spawn key values, the designer's own and
//! its bases':
//!
//! ```ignore
//! let options = EntityOptions { position: at, ..Default::default() };
//! let boss = entity::npc_trooper_boss().create_with(&options, |keys| {
//!     keys.LaneNum = Some(2);
//!     keys.globalname = Some("boss".to_owned());
//! });
//! ```
//!
//! Each field finds itself by name, so a game update that moves the field
//! needs no new build of the mod. The host refuses a handle whose entity is
//! not of the class, so a mismatched class fails instead of touching another
//! entity's memory. entity_class names the class a handle holds.
#![allow(non_camel_case_types, non_snake_case)]
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use crate::generated::{
    create_entity, fire_input, read_field, read_field_list, write_field, write_field_list, Angles,
    EntityOptions, EntityValue, FieldType, FieldValue, KeyValue, Vector,
};

/// INVALID_HANDLE is the handle of no entity.
pub const INVALID_HANDLE: u32 = 0xFFFF_FFFF;

/// CEntityInstance is the root of every entity class: one entity's handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct CEntityInstance {
    /// handle names the entity, as a pawn's entity does.
    pub handle: u32,
}

impl fmt::Debug for CEntityInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CEntityInstance({:#x})", self.handle)
    }
}

/// Class is an entity class: CEntityInstance or a class that derives from it.
pub trait Class: Copy {
    /// NAME is the class's name in the game's schema.
    const NAME: &'static str;

    /// of addresses the entity of handle as the class.
    fn of(handle: u32) -> Self;

    /// handle returns the entity's handle.
    fn handle(self) -> u32;

    /// cast addresses the entity as another class, a base or a class that
    /// derives from this one. It costs nothing and checks nothing: the host
    /// refuses a field of a class the entity is not of.
    fn cast<C: Class>(self) -> C {
        C::of(self.handle())
    }
}

impl Class for CEntityInstance {
    const NAME: &'static str = "CEntityInstance";

    fn of(handle: u32) -> Self {
        CEntityInstance { handle }
    }

    fn handle(self) -> u32 {
        self.handle
    }
}

/// class! declares the entity class $name over its base, which it
/// dereferences to, so it has its base's fields and inputs.
macro_rules! class {
    ($name:ident: $base:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct $name($base);

        impl Deref for $name {
            type Target = $base;

            fn deref(&self) -> &$base {
                &self.0
            }
        }

        impl Class for $name {
            const NAME: &'static str = stringify!($name);

            fn of(handle: u32) -> Self {
                $name(<$base as Class>::of(handle))
            }

            fn handle(self) -> u32 {
                self.handle
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:#x})", stringify!($name), self.handle)
            }
        }

        impl $name {
            /// field addresses the field name of this class on the entity.
            #[allow(dead_code)]
            fn field<A: Accessor>(self, name: &'static str) -> A {
                A::of(Place {
                    entity: self.handle,
                    class: stringify!($name),
                    name,
                })
            }
        }
    };
}

/// Place names one schema field of one entity: the class that declares it
/// and its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    /// entity is the entity's handle.
    pub entity: u32,
    /// class is the class that declares the field.
    pub class: &'static str,
    /// name is the field's schema name, such as m_iHealth.
    pub name: &'static str,
}

/// Accessor is a value that gets and sets one field: a Field, a List or a
/// Handle.
pub trait Accessor {
    /// of returns the accessor of the field at place.
    fn of(place: Place) -> Self;
}

/// Readable is a type a field holds, which the host reads as TYPE.
pub trait Readable: Sized {
    const TYPE: FieldType;

    /// read returns the value the host sent for the field.
    fn read(value: FieldValue) -> Option<Self>;
}

/// Writable is a type of field the host can write: every Readable but a
/// string.
pub trait Writable: Readable {
    /// write returns the value as the host takes it.
    fn write(&self) -> FieldValue;
}

/// number implements Readable and Writable for a numeric type. The host
/// sends a 64-bit integer as an integer and every other number as a float,
/// which holds a 64-bit integer inexactly.
macro_rules! number {
    ($($type:ty => $field:ident $case:ident $wire:ty),* $(,)?) => {$(
        impl Readable for $type {
            const TYPE: FieldType = FieldType::$field;

            fn read(value: FieldValue) -> Option<Self> {
                match value {
                    FieldValue::Number(number) => Some(number as $type),
                    FieldValue::Integer(integer) => Some(integer as $type),
                    _ => None,
                }
            }
        }

        impl Writable for $type {
            fn write(&self) -> FieldValue {
                FieldValue::$case(*self as $wire)
            }
        }
    )*};
}

number! {
    i8 => Int8 Number f64,
    i16 => Int16 Number f64,
    i32 => Int32 Number f64,
    u8 => Uint8 Number f64,
    u16 => Uint16 Number f64,
    u32 => Uint32 Number f64,
    i64 => Int64 Integer i64,
    u64 => Uint64 Integer i64,
    f32 => Float32 Number f64,
    f64 => Float64 Number f64,
}

impl Readable for bool {
    const TYPE: FieldType = FieldType::Bool;

    fn read(value: FieldValue) -> Option<Self> {
        match value {
            FieldValue::Boolean(value) => Some(value),
            _ => None,
        }
    }
}

impl Writable for bool {
    fn write(&self) -> FieldValue {
        FieldValue::Boolean(*self)
    }
}

/// A string field, which the host can read but not write.
impl Readable for String {
    const TYPE: FieldType = FieldType::String;

    fn read(value: FieldValue) -> Option<Self> {
        match value {
            FieldValue::Text(text) => Some(text),
            _ => None,
        }
    }
}

impl Readable for Vector {
    const TYPE: FieldType = FieldType::Vector;

    fn read(value: FieldValue) -> Option<Self> {
        match value {
            FieldValue::Vector(vector) => Some(vector),
            _ => None,
        }
    }
}

impl Writable for Vector {
    fn write(&self) -> FieldValue {
        FieldValue::Vector(*self)
    }
}

/// A rotation field, which the host reads as a vector of pitch, yaw and roll.
impl Readable for Angles {
    const TYPE: FieldType = FieldType::Vector;

    fn read(value: FieldValue) -> Option<Self> {
        Vector::read(value).map(|vector| Angles {
            pitch: vector.x,
            yaw: vector.y,
            roll: vector.z,
        })
    }
}

impl Writable for Angles {
    fn write(&self) -> FieldValue {
        FieldValue::Vector(Vector {
            x: self.pitch,
            y: self.yaw,
            z: self.roll,
        })
    }
}

/// A handle in a list names its entity as the root class; cast gives it the
/// class the mod knows it has.
impl Readable for CEntityInstance {
    const TYPE: FieldType = FieldType::Handle;

    fn read(value: FieldValue) -> Option<Self> {
        u32::read(value).map(CEntityInstance::of)
    }
}

impl Writable for CEntityInstance {
    fn write(&self) -> FieldValue {
        self.handle.write()
    }
}

/// Field is one field of an entity that holds a T.
pub struct Field<T> {
    place: Place,
    holds: PhantomData<fn() -> T>,
}

impl<T> Accessor for Field<T> {
    fn of(place: Place) -> Self {
        Field {
            place,
            holds: PhantomData,
        }
    }
}

impl<T> Field<T> {
    /// place returns the entity, class and name of the field.
    pub fn place(&self) -> Place {
        self.place
    }
}

impl<T: Readable> Field<T> {
    /// get reads the field, or returns None when the host cannot read it;
    /// the server log says why.
    pub fn get(&self) -> Option<T> {
        let Place {
            entity,
            class,
            name,
        } = self.place;
        read_field(entity, class, name, T::TYPE).and_then(T::read)
    }
}

impl<T: Writable> Field<T> {
    /// set writes the field and returns whether the host wrote it.
    pub fn set(&self, value: T) -> bool {
        let Place {
            entity,
            class,
            name,
        } = self.place;
        write_field(entity, class, name, T::TYPE, &value.write())
    }
}

/// List is a field that holds a list of T. A list field is a CUtlVector of
/// the game, such as the banned heroes of CCitadelGameRules.
pub struct List<T> {
    place: Place,
    holds: PhantomData<fn() -> T>,
}

impl<T> Accessor for List<T> {
    fn of(place: Place) -> Self {
        List {
            place,
            holds: PhantomData,
        }
    }
}

impl<T> List<T> {
    /// place returns the entity, class and name of the field.
    pub fn place(&self) -> Place {
        self.place
    }
}

impl<T: Readable> List<T> {
    /// get reads the elements, or returns none when the host cannot read the
    /// field.
    pub fn get(&self) -> Vec<T> {
        let Place {
            entity,
            class,
            name,
        } = self.place;
        read_field_list(entity, class, name, T::TYPE)
            .into_iter()
            .filter_map(T::read)
            .collect()
    }
}

impl<T: Writable> List<T> {
    /// set replaces the elements, growing the list when it holds fewer, and
    /// returns whether the host wrote them.
    pub fn set(&self, values: &[T]) -> bool {
        let Place {
            entity,
            class,
            name,
        } = self.place;
        let values: Vec<FieldValue> = values.iter().map(T::write).collect();
        write_field_list(entity, class, name, T::TYPE, &values)
    }
}

/// Handle is a field that names another entity, of class C.
pub struct Handle<C> {
    place: Place,
    holds: PhantomData<fn() -> C>,
}

impl<C> Accessor for Handle<C> {
    fn of(place: Place) -> Self {
        Handle {
            place,
            holds: PhantomData,
        }
    }
}

impl<C: Class> Handle<C> {
    /// place returns the entity, class and name of the field.
    pub fn place(&self) -> Place {
        self.place
    }

    /// get reads the field and addresses the entity it names. It returns
    /// None when the field names no entity or the host cannot read it.
    pub fn get(&self) -> Option<C> {
        let Place {
            entity,
            class,
            name,
        } = self.place;
        let handle = read_field(entity, class, name, FieldType::Handle).and_then(u32::read)?;
        (handle != INVALID_HANDLE).then(|| C::of(handle))
    }

    /// set points the field at entity, or at no entity for None, and returns
    /// whether the host wrote it.
    pub fn set(&self, entity: Option<C>) -> bool {
        let handle = entity.map_or(INVALID_HANDLE, Class::handle);
        let Place {
            entity,
            class,
            name,
        } = self.place;
        write_field(entity, class, name, FieldType::Handle, &handle.write())
    }
}

/// The value of a key or of an input's parameter is a case of EntityValue: a
/// bool is its boolean, an i32 its integer, an f32 its number, text its text,
/// a u32 its color as 0xRRGGBBAA, and angles travel as a vector of pitch, yaw
/// and roll.
impl From<bool> for EntityValue {
    fn from(value: bool) -> Self {
        EntityValue::Boolean(value)
    }
}

impl From<i32> for EntityValue {
    fn from(value: i32) -> Self {
        EntityValue::Integer(value)
    }
}

impl From<f32> for EntityValue {
    fn from(value: f32) -> Self {
        EntityValue::Number(value)
    }
}

impl From<&str> for EntityValue {
    fn from(value: &str) -> Self {
        EntityValue::Text(value.to_owned())
    }
}

impl From<String> for EntityValue {
    fn from(value: String) -> Self {
        EntityValue::Text(value)
    }
}

impl From<Vector> for EntityValue {
    fn from(value: Vector) -> Self {
        EntityValue::Vector(value)
    }
}

impl From<u32> for EntityValue {
    fn from(value: u32) -> Self {
        EntityValue::Color(value)
    }
}

impl From<Angles> for EntityValue {
    fn from(value: Angles) -> Self {
        EntityValue::Vector(Vector {
            x: value.pitch,
            y: value.yaw,
            z: value.roll,
        })
    }
}

/// send sends the input name to the entity of handle, with value if the
/// input takes one, caused by the entity of handle activator if the mod
/// names one.
fn send(handle: u32, name: &str, value: Option<EntityValue>, activator: Option<u32>) -> bool {
    fire_input(handle, name, value.as_ref(), activator)
}

/// Keys is a key value struct: the spawn key values of one data description
/// and its bases.
pub trait Keys {
    /// append appends the key values the struct sets to out, its base's
    /// first.
    fn append(&self, out: &mut Vec<KeyValue>);
}

/// keys! implements Keys for a key value struct from its members and the key
/// each sets, and makes the struct dereference to its base's.
macro_rules! keys {
    ($name:ident { $($member:ident => $key:literal),* }) => {
        impl Keys for $name {
            #[allow(unused_variables)]
            fn append(&self, out: &mut Vec<KeyValue>) {
                $(
                    if let Some(value) = &self.$member {
                        out.push(KeyValue { key: $key.to_owned(), value: Some(value.clone().into()) });
                    }
                )*
            }
        }
    };
    ($name:ident: $base:ident { $($member:ident => $key:literal),* }) => {
        impl Deref for $name {
            type Target = $base;

            fn deref(&self) -> &$base {
                &self.base
            }
        }

        impl DerefMut for $name {
            fn deref_mut(&mut self) -> &mut $base {
                &mut self.base
            }
        }

        impl Keys for $name {
            fn append(&self, out: &mut Vec<KeyValue>) {
                self.base.append(out);
                $(
                    if let Some(value) = &self.$member {
                        out.push(KeyValue { key: $key.to_owned(), value: Some(value.clone().into()) });
                    }
                )*
            }
        }
    };
}

/// Designer is a designer name a mod creates entities of. E is the class
/// that implements it and K its key value struct.
pub struct Designer<E, K> {
    /// name is the designer name, such as npc_trooper_boss.
    pub name: &'static str,
    /// subclass is the game data entry create gives an entity whose options
    /// name none, or empty when the entity needs none to survive its spawn.
    pub subclass: &'static str,
    makes: PhantomData<fn(K) -> E>,
}

impl<E: Class, K: Keys + Default> Designer<E, K> {
    /// new returns the Designer of name.
    pub const fn new(name: &'static str, subclass: &'static str) -> Self {
        Designer {
            name,
            subclass,
            makes: PhantomData,
        }
    }

    /// create creates an entity of the designer name from options and
    /// addresses it as the class that implements it. It returns None when
    /// the host refuses.
    pub fn create(&self, options: &EntityOptions) -> Option<E> {
        self.create_with(options, |_| {})
    }

    /// create_with creates an entity as create does, with the spawn key
    /// values set sets on the designer's key value struct. A member set leaves
    /// at None leaves its key unset.
    pub fn create_with(&self, options: &EntityOptions, set: impl FnOnce(&mut K)) -> Option<E> {
        // Pass every option through, as the mod set it, under the designer
        // name.
        let mut request = options.clone();
        request.designer_name = self.name.to_owned();

        // Give the designer's subclass to an entity whose options name none,
        // and add the typed key values after the mod's own.
        if request.subclass.is_empty() {
            request.subclass = self.subclass.to_owned();
        }
        let mut keys = K::default();
        set(&mut keys);
        keys.append(&mut request.key_values);
        create_entity(&request).map(E::of)
    }
}

#[rustfmt::skip]
#[path = "entity/classes.gen.rs"]
mod classes;
#[rustfmt::skip]
#[path = "entity/models.gen.rs"]
mod models;
#[rustfmt::skip]
#[path = "entity/abilities.gen.rs"]
mod abilities;
#[rustfmt::skip]
#[path = "entity/inputs.gen.rs"]
mod inputs;
#[rustfmt::skip]
#[path = "entity/keys.gen.rs"]
mod key_values;
#[rustfmt::skip]
#[path = "entity/designers.gen.rs"]
mod designers;

pub use abilities::*;
pub use classes::*;
pub use designers::*;
pub use key_values::*;
pub use models::*;
