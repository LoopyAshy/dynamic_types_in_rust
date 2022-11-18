use std::{sync::Arc, any::{TypeId, Any}, ptr::drop_in_place, mem::MaybeUninit};

use ahash::AHashMap;
use smartstring::alias::String;
use parking_lot::{RwLock, Mutex};
use thiserror::Error;

#[derive(Default)]
pub struct TypeRegistry {
    static_types: RwLock<AHashMap<TypeId, Arc<StaticTypeLayout>>>,
    dynamic_types: RwLock<AHashMap<String, Arc<DynamicTypeLayout>>>,
}

impl TypeRegistry {
    pub fn add<T: 'static + DefaultBytes>(&self) {
        self.static_types
            .write()
            .insert(TypeId::of::<T>(), Arc::new(StaticTypeLayout::of::<T>()));
    }

    pub fn add_all<T: 'static + DefaultBytes>(&self) {
        macro_rules! add_type {
            ($this:ident, $ty:ty) => {
                $this.insert(
                    TypeId::of::<$ty>(),
                    Arc::new(StaticTypeLayout::of::<$ty>()),
                );
                add_type!(vec, $this, $ty);
            };
            (vec, $this:ident, $ty:ty) => {
                $this.insert(
                    TypeId::of::<Vec<$ty>>(),
                    Arc::new(StaticTypeLayout::of::<Vec<$ty>>()),
                );
                $this.insert(
                    TypeId::of::<Arc<Vec<$ty>>>(),
                    Arc::new(StaticTypeLayout::of::<Arc<Vec<$ty>>>()),
                );
                $this.insert(
                    TypeId::of::<RwLock<Vec<$ty>>>(),
                    Arc::new(StaticTypeLayout::of::<RwLock<Vec<$ty>>>()),
                );
                $this.insert(
                    TypeId::of::<Mutex<Vec<$ty>>>(),
                    Arc::new(StaticTypeLayout::of::<Mutex<Vec<$ty>>>()),
                );
                $this.insert(
                    TypeId::of::<RwLock<Vec<Option<$ty>>>>(),
                    Arc::new(StaticTypeLayout::of::<RwLock<Vec<Option<$ty>>>>()),
                );
                $this.insert(
                    TypeId::of::<Mutex<Vec<Option<$ty>>>>(),
                    Arc::new(StaticTypeLayout::of::<Mutex<Vec<Option<$ty>>>>()),
                );
            };
        }

        let mut this = self.static_types.write();
        add_type!(this, T);
        add_type!(this, Option<T>);
        add_type!(this, Arc<T>);
        add_type!(this, Arc<RwLock<T>>);
        add_type!(this, Arc<Mutex<T>>);
    }

    pub fn add_dyn(&self, layout: DynamicTypeLayout) {
        self.dynamic_types
            .write()
            .insert(layout.name.clone(), Arc::new(layout));
    }

    pub fn get_static_layout<T: 'static + DefaultBytes>(&self) -> Arc<StaticTypeLayout> {
        if let Some(v) = self.static_types.read().get(&TypeId::of::<T>()).cloned() {
            return v;
        }

        let ty = Arc::new(StaticTypeLayout::of::<T>());

        self.static_types
            .write()
            .insert(TypeId::of::<T>(), ty.clone());

        ty
    }

    pub fn get_dynamic_layout(&self, name: &str) -> Option<Arc<DynamicTypeLayout>> {
        self.dynamic_types.read().get(name).cloned()
    }

    pub fn create_dynamic(&self, name: &str) -> DynamicStruct {
        DynamicStruct::new(
            self.get_dynamic_layout(name)
                .unwrap_or_else(|| panic!("No dynamic type with name {}", name)),
        )
    }
}

#[derive(Debug, Error)]
pub enum DynamicFieldError<T> {
    #[error("Field index {index} requested was out of bounds.")]
    FieldGetIndexOutOfBounds {
        index: usize
    },
    #[error("Field {name} does not exist.")]
    GetFieldNameNotFound {
        name: String
    },
    #[error("Field requested with incorrect type: requested: {type_requested}, expected: {actual_type}")]
    GetInvalidTypeOfField {
        type_requested: String,
        actual_type: String
    },
    #[error("Field {name} does not exist.")]
    SetFieldNameNotFound {
        name: String,
        value: T
    },
    #[error("Field index {index} requested was out of bounds.")]
    FieldSetIndexOutOfBounds {
        value: T,
        index: usize
    },
    #[error("Field requested with incorrect type: requested: {type_requested}, expected: {actual_type}")]
    SetInvalidTypeOfField {
        value: T,
        type_requested: String,
        actual_type: String
    }
}

pub struct DynamicTypeLayout {
    pub name: String,
    pub field_types: Vec<TypeId>,
    pub field_offsets: Vec<usize>,
    pub field_sizes: Vec<usize>,
    pub field_defaults: Vec<unsafe fn() -> Vec<u8>>,
    pub field_drop_fns: Vec<Option<fn(*const u8)>>,
    pub name_to_index: AHashMap<std::string::String, usize>,
    pub total_size: usize,
    pub field_type_names: Vec<&'static str>,
}

impl DynamicTypeLayout {
    pub fn new(name: String, fields: &[(&str, &StaticTypeLayout)]) -> Self {
        let mut field_types = Vec::with_capacity(fields.len());
        let mut field_offsets = Vec::with_capacity(fields.len());
        let mut field_sizes = Vec::with_capacity(fields.len());
        let mut name_to_index = AHashMap::with_capacity(fields.len());
        let mut field_type_names = Vec::with_capacity(fields.len());
        let mut field_defaults = Vec::with_capacity(fields.len());
        let mut field_drop_fns = Vec::with_capacity(fields.len());
        let mut total_size = 0;

        let mut offset = 0;
        for (index, field) in fields.iter().enumerate() {
            if field_type_names.contains(&field.0) {
                panic!("Same field name {} declared multiple times.", field.0);
            }
            field_types.push(field.1.type_id);
            let remainder = offset % field.1.align;
            if remainder != 0 {
                offset += field.1.align - remainder;
            }
            field_offsets.push(offset);
            total_size += field.1.size;
            field_sizes.push(field.1.size);
            name_to_index.insert(field.0.into(), index);
            offset += field.1.size;

            field_type_names.push(field.1.name);
            field_defaults.push(field.1.default);
            field_drop_fns.push(field.1.drop_fn);
        }
        total_size = total_size + (offset % total_size);

        Self {
            name,
            field_types,
            field_offsets,
            field_sizes,
            name_to_index,
            total_size,
            field_type_names,
            field_defaults,
            field_drop_fns,
        }
    }

    #[inline]
    pub fn set_field<T: 'static>(&self, data: &mut [u8], name: &str, val: T) {
        let index = self.name_to_index[name];
        self.set_field_by_index(data, index, val);
    }

    #[inline]
    pub fn set_field_by_index<T: 'static>(&self, data: &mut [u8], index: usize, val: T) {
        self.check_type::<T>(index);
        unsafe {
            self.set_field_unchecked_by_index(data, index, val);
        }
    }

    #[inline]
    pub fn try_set_field<T: 'static>(&self, data: &mut [u8], name: &str, val: T) -> Result<(), DynamicFieldError<T>> {
        if let Some(index) = self.name_to_index.get(name) {
            self.try_set_field_by_index(data, *index, val)
        } else {
            Err(DynamicFieldError::SetFieldNameNotFound { name: name.into(), value: val })
        }
    }

    #[inline]
    pub fn try_set_field_by_index<T: 'static>(&self, data: &mut [u8], index: usize, val: T) -> Result<(), DynamicFieldError<T>> {
        if self.type_is::<T>(index) {
            if self.field_offsets.len() < index {
                Err(DynamicFieldError::FieldSetIndexOutOfBounds { index, value: val })
            } else {
                unsafe {
                    self.set_field_unchecked_by_index(data, index, val);
                }
                Ok(())
            }
        } else {
            Err(DynamicFieldError::SetInvalidTypeOfField { value: val, type_requested: std::any::type_name::<T>().into(), actual_type: self.field_type_names[index].to_string().into() })
        }
    }

    #[inline]
    fn check_type<T: 'static>(&self, index: usize) {
        if !self.type_is::<T>(index) {
            panic!(
                "Invalid type, expected: {:?}, but found {:?}",
                self.field_type_names[index],
                std::any::type_name::<T>()
            );
        }
    }

    #[inline]
    fn type_is<T: 'static>(&self, index: usize) -> bool {
        self.field_types[index] == TypeId::of::<T>()
    }

    #[inline]
    pub fn clone_field<T: 'static + Clone>(&self, data: &[u8], name: &str) -> T {
        let index = self.name_to_index[name];
        self.clone_field_by_index(data, index)
    }

    #[inline]
    pub fn clone_field_by_index<T: 'static + Clone>(&self, data: &[u8], index: usize) -> T {
        self.check_type::<T>(index);
        unsafe { self.clone_field_unchecked_by_index(data, index) }
    }

    #[inline]
    //TODO: add error type
    pub fn try_clone_field<T: 'static + Clone>(&self, data: &[u8], name: &str) -> Result<T, DynamicFieldError<()>> {
        if let Some(index) = self.name_to_index.get(name) {
            self.try_clone_field_by_index(data, *index)
        } else {
            Err(DynamicFieldError::GetFieldNameNotFound { name: name.into() })
        }
    }

    #[inline]
    //TODO: add error type
    pub fn try_clone_field_by_index<T: 'static + Clone>(&self, data: &[u8], index: usize) -> Result<T, DynamicFieldError<()>> {
        if self.type_is::<T>(index) {
            if self.field_offsets.len() < index {
                Err(DynamicFieldError::FieldGetIndexOutOfBounds { index })
            } else {
                Ok(unsafe { self.clone_field_unchecked_by_index(data, index) })
            }
        } else {
            Err(DynamicFieldError::GetInvalidTypeOfField { type_requested: std::any::type_name::<T>().into(), actual_type: self.field_type_names[index].to_string().into() })
        }
    }

    #[inline]
    pub fn get_field_ref<T: 'static>(&self, data: &[u8], name: &str) -> &T {
        let index = self.name_to_index[name];
        self.get_field_ref_by_index(data, index)
    }

    #[inline]
    pub fn get_field_ref_by_index<T: 'static>(&self, data: &[u8], index: usize) -> &T {
        self.check_type::<T>(index);
        unsafe { self.get_field_ref_unchecked_by_index(data, index) }
    }

    #[inline]
    //TODO: add error type
    pub fn try_get_field_ref<T: 'static>(&self, data: &[u8], name: &str) -> Result<&T, DynamicFieldError<()>> {
        if let Some(index) = self.name_to_index.get(name) {
            self.try_get_field_ref_by_index(data, *index)
        } else {
            Err(DynamicFieldError::GetFieldNameNotFound { name: name.into() })
        }
    }

    #[inline]
    //TODO: add error type
    pub fn try_get_field_ref_by_index<T: 'static>(&self, data: &[u8], index: usize) -> Result<&T, DynamicFieldError<()>> {
        if self.type_is::<T>(index) {
            if self.field_offsets.len() < index {
                Err(DynamicFieldError::FieldGetIndexOutOfBounds { index })
            } else {
                Ok(unsafe { self.get_field_ref_unchecked_by_index(data, index) })
            }
        } else {
            Err(DynamicFieldError::GetInvalidTypeOfField { type_requested: std::any::type_name::<T>().into(), actual_type: self.field_type_names[index].to_string().into() })
        }
    }

    #[inline]
    pub fn get_field_mut<T: 'static>(&self, data: &mut [u8], name: &str) -> &mut T {
        let index = self.name_to_index[name];
        self.get_field_mut_by_index(data, index)
    }

    #[allow(clippy::mut_from_ref)]
    #[inline]
    pub fn get_field_mut_by_index<T: 'static>(&self, data: &mut [u8], index: usize) -> &mut T {
        self.check_type::<T>(index);
        unsafe { self.get_field_mut_unchecked_by_index(data, index) }
    }

    #[inline]
    //TODO: add error type
    pub fn try_get_field_mut<T: 'static>(&self, data: &mut [u8], name: &str) -> Result<&mut T, DynamicFieldError<()>> {
        if let Some(index) = self.name_to_index.get(name) {
            self.try_get_field_mut_by_index(data, *index)
        } else {
            Err(DynamicFieldError::GetFieldNameNotFound { name: name.into() })
        }
    }

    #[inline]
    //TODO: add error type
    pub fn try_get_field_mut_by_index<T: 'static>(&self, data: &mut [u8], index: usize) -> Result<&mut T, DynamicFieldError<()>> {
        if self.type_is::<T>(index) {
            if self.field_offsets.len() < index {
                Err(DynamicFieldError::FieldGetIndexOutOfBounds { index })
            } else {
                Ok(unsafe { self.get_field_mut_unchecked_by_index(data, index) })
            }
        } else {
            Err(DynamicFieldError::GetInvalidTypeOfField { type_requested: std::any::type_name::<T>().into(), actual_type: self.field_type_names[index].to_string().into() })
        }
    }

    #[inline]
    /// # Safety
    /// The field's type must match the generic type `T`
    pub unsafe fn set_field_unchecked_by_index<T: 'static>(
        &self,
        data: &mut [u8],
        index: usize,
        val: T,
    ) {
        let mut ptr = data.as_mut_ptr();
        ptr = ptr.add(self.field_offsets[index]);
        let ptr = ptr.cast::<T>();
        *ptr = val;
    }

    #[inline]
    /// # Safety
    /// The field's type must match the generic type `T`
    pub unsafe fn clone_field_unchecked_by_index<T: 'static + Clone>(
        &self,
        data: &[u8],
        index: usize,
    ) -> T {
        let offset = self.field_offsets[index];
        let data = data.as_ptr().add(offset);
        (*(std::mem::transmute::<*const u8, *const T>(data))).clone()
    }

    #[inline]
    /// # Safety
    /// The field's type must match the generic type `T`
    pub unsafe fn get_field_ref_unchecked_by_index<T: 'static>(
        &self,
        data: &[u8],
        index: usize,
    ) -> &T {
        let offset = self.field_offsets[index];
        let data = data.as_ptr().add(offset);
        &*std::mem::transmute::<*const u8, *const T>(data)
    }

    #[inline]
    #[allow(clippy::mut_from_ref)]
    /// # Safety
    /// The field's type must match the generic type `T`
    pub unsafe fn get_field_mut_unchecked_by_index<T: 'static>(
        &self,
        data: &mut [u8],
        index: usize,
    ) -> &mut T {
        let offset = self.field_offsets[index];
        let data = data.as_ptr().add(offset);
        &mut *std::mem::transmute::<*const u8, *mut T>(data)
    }
}

pub struct DynamicStruct {
    type_layout: Arc<DynamicTypeLayout>,
    data: Vec<u8>,
}

impl Drop for DynamicStruct {
    fn drop(&mut self) {
        //Data has been taken or it is a 0 sized type.
        if self.data.is_empty() {
            return;
        }
        for (index, field) in self.type_layout.field_drop_fns.iter().enumerate() {
            if let Some(drop) = field {
                let offset = self.type_layout.field_offsets[index];
                // Make sure we are not double dropping data in the default.
                let ptr = self.data[offset..].as_ptr();
                drop(ptr);
            }
        }
    }
}

impl DynamicStruct {
    pub fn new(type_layout: Arc<DynamicTypeLayout>) -> Self {
        let mut data = vec![0u8; type_layout.total_size];

        for (create, offset) in type_layout
            .field_defaults
            .iter()
            .zip(type_layout.field_offsets.iter())
        {
            let bytes = unsafe { create() };
            let slice = &mut data[*offset..];
            for (index, byte) in bytes.iter().enumerate() {
                slice[index] = *byte;
            }
        }

        Self { data, type_layout }
    }

    pub fn size_of(&self) -> usize {
        self.type_layout.total_size
    }

    /// # Safety
    /// Only call this if the type is identical to the dynamic types byte layout.
    #[inline]
    pub unsafe fn cast<T>(mut self) -> T {
        if self.data.len() != std::mem::size_of::<T>() {
            panic!("Invalid sized type, data is {} bytes large and type attempted to cast to is {} bytes large.", self.data.len(), std::mem::size_of::<T>());
        }
        let bytes = self.data.clone();
        self.data.clear();
        bytes.cast()
    }

    #[inline]
    pub fn set_field<T: 'static>(&mut self, name: &str, val: T) {
        self.type_layout.set_field(&mut self.data, name, val);
    }

    #[inline]
    pub fn clone_field<T: 'static + Clone>(&self, name: &str) -> T {
        self.type_layout.clone_field(&self.data, name)
    }

    #[inline]
    pub fn get_field_ref<T: 'static>(&self, name: &str) -> &T {
        self.type_layout.get_field_ref(self.data.as_slice(), name)
    }

    #[inline]
    pub fn get_field_mut<T: 'static>(&mut self, name: &str) -> &mut T {
        self.type_layout
            .get_field_mut(self.data.as_mut_slice(), name)
    }

    #[inline]
    pub fn set_field_by_index<T: 'static>(&mut self, val: T, index: usize) {
        self.type_layout
            .set_field_by_index(&mut self.data, index, val);
    }

    #[inline]
    pub fn clone_field_by_index<T: 'static + Clone>(&self, index: usize) -> T {
        self.type_layout.clone_field_by_index(&self.data, index)
    }

    #[inline]
    pub fn get_field_ref_by_index<T: 'static>(&self, index: usize) -> &T {
        self.type_layout
            .get_field_ref_by_index(self.data.as_slice(), index)
    }

    #[inline]
    pub fn get_field_mut_by_index<T: 'static>(&mut self, index: usize) -> &mut T {
        self.type_layout
            .get_field_mut_by_index(self.data.as_mut_slice(), index)
    }

    #[inline]
    pub fn try_set_field<T: 'static>(&mut self, name: &str, val: T) -> Result<(), DynamicFieldError<T>> {
        self.type_layout.try_set_field(&mut self.data, name, val)
    }

    #[inline]
    pub fn try_clone_field<T: 'static + Clone>(&self, name: &str) -> Result<T, DynamicFieldError<()>> {
        self.type_layout.try_clone_field(&self.data, name)
    }

    #[inline]
    pub fn try_get_field_ref<T: 'static>(&self, name: &str) -> Result<&T, DynamicFieldError<()>> {
        self.type_layout.try_get_field_ref(self.data.as_slice(), name)
    }

    #[inline]
    pub fn try_get_field_mut<T: 'static>(&mut self, name: &str) -> Result<&mut T, DynamicFieldError<()>> {
        self.type_layout
            .try_get_field_mut(self.data.as_mut_slice(), name)
    }

    #[inline]
    pub fn try_set_field_by_index<T: 'static>(&mut self, val: T, index: usize) -> Result<(), DynamicFieldError<T>> {
        self.type_layout
            .try_set_field_by_index(&mut self.data, index, val)
    }

    #[inline]
    pub fn try_clone_field_by_index<T: 'static + Clone>(&self, index: usize) -> Result<T, DynamicFieldError<()>> {
        self.type_layout.try_clone_field_by_index(&self.data, index)
    }

    #[inline]
    pub fn try_get_field_ref_by_index<T: 'static>(&self, index: usize) -> Result<&T, DynamicFieldError<()>> {
        self.type_layout
            .try_get_field_ref_by_index(self.data.as_slice(), index)
    }

    #[inline]
    pub fn try_get_field_mut_by_index<T: 'static>(&mut self, index: usize) -> Result<&mut T, DynamicFieldError<()>> {
        self.type_layout
            .try_get_field_mut_by_index(self.data.as_mut_slice(), index)
    }

}

#[derive(Debug, Clone)]
pub struct StaticTypeLayout {
    type_id: TypeId,
    size: usize,
    align: usize,
    default: unsafe fn() -> Vec<u8>,
    drop_fn: Option<fn(*const u8)>,
    name: &'static str,
}

impl StaticTypeLayout {
    pub fn of<T: Any + Default + DefaultBytes>() -> Self {
        StaticTypeLayout {
            type_id: TypeId::of::<T>(),
            size: std::mem::size_of::<T>(),
            align: std::mem::align_of::<T>(),
            default: { T::default_bytes },
            name: std::any::type_name::<T>(),
            drop_fn: {
                if std::mem::needs_drop::<T>() {
                    let func = unsafe {
                        std::mem::transmute::<unsafe fn(*mut T), fn(*const u8)>(drop_in_place::<T>)
                    };
                    Some(func)
                } else {
                    None
                }
            },
        }
    }
}

pub trait DefaultBytes: Default {
    /// # Safety
    /// If this type is not `Copy` then remember to cast it back to its original type and drop it when you're done with it.
    unsafe fn default_bytes() -> Vec<u8> {
        let new = Self::default();
        let new_bytes = unsafe {
            std::slice::from_raw_parts(
                &new as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        };
        let mut output = Vec::with_capacity(std::mem::size_of::<Self>());
        for b in new_bytes {
            output.push(*b);
        }
        if std::mem::needs_drop::<Self>() {
            std::mem::forget(new);
        }
        output
    }
}

impl<T: Default> DefaultBytes for T {}

/// # Safety
/// Only a valid trait for sequential collections of bytes.
pub unsafe trait VecToType {
    unsafe fn cast<T>(self) -> T;
    unsafe fn drop_as<T>(self);
}

unsafe impl VecToType for Vec<u8> {
    /// # Safety
    /// Only cast the vec if it was created with `DefaultBytes` trait or created manually the same way and with the same generic type.
    unsafe fn cast<T>(self) -> T {
        let bytes = self.as_ptr().cast::<T>();
        let mut output = MaybeUninit::<T>::zeroed();
        bytes.copy_to_nonoverlapping(output.as_mut_ptr(), 1);
        output.assume_init()
    }

    /// # Safety
    /// Only call this if it was created with `DefaultBytes` trait or created manually the same way and with the same generic type.
    unsafe fn drop_as<T>(self) {
        let bytes = self.as_ptr().cast_mut().cast::<T>();
        drop_in_place(bytes);
    }
}
