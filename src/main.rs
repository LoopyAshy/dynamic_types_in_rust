#![feature(vec_into_raw_parts)]

use std::{
    any::{Any, TypeId},
    hint::black_box,
    mem::MaybeUninit,
    ptr::drop_in_place,
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use parking_lot::{Mutex, RwLock};
use smartstring::alias::String;

fn main() {
    let mut type_registry = TypeRegistry {
        static_types: RwLock::default(),
        dynamic_types: RwLock::default(),
    };

    let type_layout = DynTypeLayout::new(
        "Test".into(),
        &[
            ("o", &type_registry.get_static_layout::<u8>()),
            ("k", &type_registry.get_static_layout::<u8>()),
            ("a", &type_registry.get_static_layout::<i32>()),
            ("b", &type_registry.get_static_layout::<f32>()),
            ("c", &type_registry.get_static_layout::<String>()),
            ("d", &type_registry.get_static_layout::<Vec<i32>>()),
            ("e", &type_registry.get_static_layout::<Arc<TestCrap>>()),
        ],
    );

    type_registry.add_dyn(type_layout);

    let mut dyn_type = type_registry.create_dynamic("Test");

    dyn_type.set_field("a", 1337i32);
    dyn_type.set_field("b", 5f32);
    dyn_type.set_field("c", String::from("Hello World"));
    dyn_type.set_field("d", vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    dyn_type.set_field("e", Arc::new(TestCrap));

    let mut timer = TimeCollection::with_capacity(100000);
    for _ in 0..100000 {
        timer.start();
        let _a = black_box(dyn_type.get_field_ref_by_index::<i32>(2));
        let _b = black_box(dyn_type.get_field_ref_by_index::<f32>(3));
        let _c = black_box(dyn_type.get_field_ref_by_index::<String>(4));
        let _d = black_box(dyn_type.get_field_ref_by_index::<Vec<i32>>(5));
        timer.end();
    }

    let index_average = timer.average();

    timer.clear();

    for _ in 0..100000 {
        timer.start();
        let _a = black_box(dyn_type.get_field_ref::<i32>("a"));
        let _b = black_box(dyn_type.get_field_ref::<f32>("b"));
        let _c = black_box(dyn_type.get_field_ref::<String>("c"));
        let _d = black_box(dyn_type.get_field_ref::<Vec<i32>>("d"));
        timer.end();
    }

    let name_average = timer.average();

    timer.clear();

    #[derive(Debug)]
    #[repr(C)]
    pub struct TestLayout {
        o: u8,
        k: u8,
        a: i32,
        b: f32,
        c: String,
        d: Vec<i32>,
        e: Arc<TestCrap>,
    }

    let data = unsafe { dyn_type.cast::<TestLayout>() };
    for _ in 0..100000 {
        timer.start();
        let _a = black_box(&data.a);
        let _b = black_box(&data.b);
        let _c = black_box(&data.c);
        let _d = black_box(&data.d);
        timer.end();
    }

    let casted_average = timer.average();

    println!(
        "name get: {:?}, index get: {:?}, casted get: {:?}",
        name_average, index_average, casted_average
    );

    println!("{:?}", data);
}

struct TimeCollection {
    times: Vec<Duration>,
    current_time: Instant,
}

impl TimeCollection {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            times: Vec::with_capacity(capacity),
            current_time: Instant::now(),
        }
    }

    pub fn start(&mut self) {
        self.current_time = Instant::now();
    }

    pub fn end(&mut self) {
        self.times.push(self.current_time.elapsed());
    }

    pub fn average(&self) -> Duration {
        self.times
            .iter()
            .sum::<Duration>()
            .div_f64(self.times.len() as f64)
    }

    pub fn clear(&mut self) {
        self.times.clear();
    }
}

pub struct TypeRegistry {
    static_types: RwLock<AHashMap<TypeId, Arc<DynamicTypeLayout>>>,
    dynamic_types: RwLock<AHashMap<String, Arc<DynTypeLayout>>>,
}

impl TypeRegistry {
    pub fn add<T: 'static + DefaultBytes>(&self) {
        self.static_types
            .write()
            .insert(TypeId::of::<T>(), Arc::new(DynamicTypeLayout::of::<T>()));
    }

    pub fn add_all<T: 'static + DefaultBytes>(&self) {
        macro_rules! add_type {
            ($this:ident, $ty:ty) => {
                $this.insert(
                    TypeId::of::<$ty>(),
                    Arc::new(DynamicTypeLayout::of::<$ty>()),
                );
                add_type!(vec, $this, $ty);
            };
            (vec, $this:ident, $ty:ty) => {
                $this.insert(
                    TypeId::of::<Vec<$ty>>(),
                    Arc::new(DynamicTypeLayout::of::<Vec<$ty>>()),
                );
                $this.insert(
                    TypeId::of::<Arc<Vec<$ty>>>(),
                    Arc::new(DynamicTypeLayout::of::<Arc<Vec<$ty>>>()),
                );
                $this.insert(
                    TypeId::of::<RwLock<Vec<$ty>>>(),
                    Arc::new(DynamicTypeLayout::of::<RwLock<Vec<$ty>>>()),
                );
                $this.insert(
                    TypeId::of::<Mutex<Vec<$ty>>>(),
                    Arc::new(DynamicTypeLayout::of::<Mutex<Vec<$ty>>>()),
                );
                $this.insert(
                    TypeId::of::<RwLock<Vec<Option<$ty>>>>(),
                    Arc::new(DynamicTypeLayout::of::<RwLock<Vec<Option<$ty>>>>()),
                );
                $this.insert(
                    TypeId::of::<Mutex<Vec<Option<$ty>>>>(),
                    Arc::new(DynamicTypeLayout::of::<Mutex<Vec<Option<$ty>>>>()),
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

    pub fn add_dyn(&self, layout: DynTypeLayout) {
        self.dynamic_types
            .write()
            .insert(layout.name.clone(), Arc::new(layout));
    }

    pub fn get_static_layout<T: 'static + DefaultBytes>(&self) -> Arc<DynamicTypeLayout> {
        if let Some(v) = self.static_types.read().get(&TypeId::of::<T>()).cloned() {
            return v;
        }

        let ty = Arc::new(DynamicTypeLayout::of::<T>());

        self.static_types
            .write()
            .insert(TypeId::of::<T>(), ty.clone());

        ty
    }

    pub fn get_dynamic_layout(&self, name: &str) -> Option<Arc<DynTypeLayout>> {
        self.dynamic_types.read().get(name).cloned()
    }

    pub fn create_dynamic(&self, name: &str) -> DynamicStruct {
        DynamicStruct::new(
            self.get_dynamic_layout(name)
                .unwrap_or_else(|| panic!("No dynamic type with name {}", name)),
        )
    }
}

pub fn kitype_to_rusttype(ctype: &str) -> &'static str {
    use std::any::type_name;
    if ctype.starts_with("class SharedPointer") {
        let ctype = ctype
            .trim_start_matches("class SharedPointer<")
            .trim_end_matches('>');
        match ctype {
            "unsigned char" => type_name::<Option<Arc<u8>>>(),
            "char" => type_name::<Option<Arc<i8>>>(),
            "short" => type_name::<Option<Arc<i16>>>(),
            "unsigned short" => type_name::<Option<Arc<u16>>>(),
            "int" => type_name::<Option<Arc<i32>>>(),
            "unsigned int" => type_name::<Option<Arc<u32>>>(),
            "long" => type_name::<Option<Arc<i32>>>(),
            "unsigned long" => type_name::<Option<Arc<u32>>>(),
            "gid" => type_name::<Option<Arc<GID>>>(),
            "float" => type_name::<Option<Arc<f32>>>(),
            "double" => type_name::<Option<Arc<f64>>>(),
            "std::string" => type_name::<Option<Arc<String>>>(),
            "std::wstring" => type_name::<Option<Arc<String>>>(),
            "class Vector3D" => type_name::<Option<Arc<Vector3D>>>(),
            "class Color" => type_name::<Option<Arc<Color>>>(),
            "class Point" => type_name::<Option<Arc<Point>>>(),
            _ => "unknown",
        }
    } else if ctype.ends_with('*') {
        let ctype = ctype.trim_end_matches('*');
        match ctype {
            "unsigned char" => type_name::<Option<Box<u8>>>(),
            "char" => type_name::<Option<Box<i8>>>(),
            "short" => type_name::<Option<Box<i16>>>(),
            "unsigned short" => type_name::<Option<Box<u16>>>(),
            "int" => type_name::<Option<Box<i32>>>(),
            "unsigned int" => type_name::<Option<Box<u32>>>(),
            "long" => type_name::<Option<Box<i32>>>(),
            "unsigned long" => type_name::<Option<Box<u32>>>(),
            "gid" => type_name::<Option<Box<GID>>>(),
            "float" => type_name::<Option<Box<f32>>>(),
            "double" => type_name::<Option<Box<f64>>>(),
            "std::string" => type_name::<Option<Box<String>>>(),
            "std::wstring" => type_name::<Option<Box<String>>>(),
            "class Vector3D" => type_name::<Option<Box<Vector3D>>>(),
            "class Color" => type_name::<Option<Box<Color>>>(),
            "class Point" => type_name::<Option<Box<Point>>>(),
            _ => "unknown",
        }
    } else {
        match ctype {
            "unsigned char" => type_name::<u8>(),
            "char" => type_name::<i8>(),
            "short" => type_name::<i16>(),
            "unsigned short" => type_name::<u16>(),
            "int" => type_name::<i32>(),
            "unsigned int" => type_name::<u32>(),
            "long" => type_name::<i32>(),
            "unsigned long" => type_name::<u32>(),
            "gid" => type_name::<GID>(),
            "float" => type_name::<f32>(),
            "double" => type_name::<f64>(),
            "std::string" => type_name::<String>(),
            "std::wstring" => type_name::<String>(),
            "class Vector3D" => type_name::<Vector3D>(),
            "class Color" => type_name::<Color>(),
            "class Point" => type_name::<Point>(),
            _ => "unknown",
        }
    }
}

pub fn kitype_to_dyn_type_layout(ctype: &str) -> DynamicTypeLayout {
    if ctype.starts_with("class SharedPointer") {
        //Shared pointers aka Arcs
        let ctype = ctype
            .trim_start_matches("class SharedPointer<")
            .trim_end_matches('>');
        match ctype {
            "unsigned char" => DynamicTypeLayout::of::<Option<Arc<u8>>>(),
            "char" => DynamicTypeLayout::of::<Option<Arc<i8>>>(),
            "short" => DynamicTypeLayout::of::<Option<Arc<i16>>>(),
            "unsigned short" => DynamicTypeLayout::of::<Option<Arc<u16>>>(),
            "int" => DynamicTypeLayout::of::<Option<Arc<i32>>>(),
            "unsigned int" => DynamicTypeLayout::of::<Option<Arc<u32>>>(),
            "long" => DynamicTypeLayout::of::<Option<Arc<i32>>>(),
            "unsigned long" => DynamicTypeLayout::of::<Option<Arc<u32>>>(),
            "gid" => DynamicTypeLayout::of::<Option<Arc<GID>>>(),
            "float" => DynamicTypeLayout::of::<Option<Arc<f32>>>(),
            "double" => DynamicTypeLayout::of::<Option<Arc<f64>>>(),
            "std::string" => DynamicTypeLayout::of::<Option<Arc<String>>>(),
            "std::wstring" => DynamicTypeLayout::of::<Option<Arc<String>>>(),
            "class Vector3D" => DynamicTypeLayout::of::<Option<Arc<Vector3D>>>(),
            "class Color" => DynamicTypeLayout::of::<Option<Arc<Color>>>(),
            "class Point" => DynamicTypeLayout::of::<Option<Arc<Point>>>(),
            _ => panic!("Unhandled type: {}", ctype),
        }
    } else if ctype.ends_with('*') {
        //Raw pointers
        let ctype = ctype.trim_end_matches('*');
        match ctype {
            "unsigned char" => DynamicTypeLayout::of::<Option<Box<u8>>>(),
            "char" => DynamicTypeLayout::of::<Option<Box<i8>>>(),
            "short" => DynamicTypeLayout::of::<Option<Box<i16>>>(),
            "unsigned short" => DynamicTypeLayout::of::<Option<Box<u16>>>(),
            "int" => DynamicTypeLayout::of::<Option<Box<i32>>>(),
            "unsigned int" => DynamicTypeLayout::of::<Option<Box<u32>>>(),
            "long" => DynamicTypeLayout::of::<Option<Box<i32>>>(),
            "unsigned long" => DynamicTypeLayout::of::<Option<Box<u32>>>(),
            "gid" => DynamicTypeLayout::of::<Option<Box<GID>>>(),
            "float" => DynamicTypeLayout::of::<Option<Box<f32>>>(),
            "double" => DynamicTypeLayout::of::<Option<Box<f64>>>(),
            "std::string" => DynamicTypeLayout::of::<Option<Box<String>>>(),
            "std::wstring" => DynamicTypeLayout::of::<Option<Box<String>>>(),
            "class Vector3D" => DynamicTypeLayout::of::<Option<Box<Vector3D>>>(),
            "class Color" => DynamicTypeLayout::of::<Option<Box<Color>>>(),
            "class Point" => DynamicTypeLayout::of::<Option<Box<Point>>>(),
            _ => panic!("Unhandled type: {}", ctype),
        }
    } else {
        match ctype {
            //Value types
            "unsigned char" => DynamicTypeLayout::of::<u8>(),
            "char" => DynamicTypeLayout::of::<i8>(),
            "short" => DynamicTypeLayout::of::<i16>(),
            "unsigned short" => DynamicTypeLayout::of::<u16>(),
            "int" => DynamicTypeLayout::of::<i32>(),
            "unsigned int" => DynamicTypeLayout::of::<u32>(),
            "long" => DynamicTypeLayout::of::<i32>(),
            "unsigned long" => DynamicTypeLayout::of::<u32>(),
            "gid" => DynamicTypeLayout::of::<GID>(),
            "float" => DynamicTypeLayout::of::<f32>(),
            "double" => DynamicTypeLayout::of::<f64>(),
            "std::string" => DynamicTypeLayout::of::<String>(),
            "std::wstring" => DynamicTypeLayout::of::<String>(),
            "class Vector3D" => DynamicTypeLayout::of::<Vector3D>(),
            "class Color" => DynamicTypeLayout::of::<Color>(),
            "class Point" => DynamicTypeLayout::of::<Point>(),
            _ => panic!("Unhandled type: {}", ctype),
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct Vector3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct Color {
    pub r: u8,
    pub b: u8,
    pub g: u8,
    pub a: u8,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct GID {
    pub id: u32,
    pub ty: u32,
}

#[derive(Debug, Default)]
pub struct TestCrap;

pub struct DynTypeLayout {
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

impl DynTypeLayout {
    pub fn new(name: String, fields: &[(&str, &DynamicTypeLayout)]) -> Self {
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
    fn check_type<T: 'static>(&self, index: usize) {
        if self.field_types[index] != TypeId::of::<T>() {
            panic!(
                "Invalid type, expected: {:?}, but found {:?}",
                self.field_type_names[index],
                std::any::type_name::<T>()
            );
        }
    }

    #[inline]
    pub fn get_field<T: 'static + Clone>(&self, data: &[u8], name: &str) -> T {
        let index = self.name_to_index[name];
        self.get_field_by_index(data, index)
    }

    #[inline]
    pub fn get_field_by_index<T: 'static + Clone>(&self, data: &[u8], index: usize) -> T {
        self.check_type::<T>(index);
        unsafe { self.get_field_unchecked_by_index(data, index) }
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
    pub unsafe fn get_field_unchecked_by_index<T: 'static + Clone>(
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
    type_layout: Arc<DynTypeLayout>,
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
    pub fn new(type_layout: Arc<DynTypeLayout>) -> Self {
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

    /// # Safety
    /// Only call this if the type is identical to the dynamic types byte layout.
    #[inline]
    pub unsafe fn cast<T>(mut self) -> T {
        let bytes = self.data.clone();
        self.data.clear();
        bytes.cast()
    }

    #[inline]
    pub fn set_field<T: 'static>(&mut self, name: &str, val: T) {
        self.type_layout.set_field(&mut self.data, name, val);
    }

    #[inline]
    pub fn get_field<T: 'static + Clone>(&self, name: &str) -> T {
        self.type_layout.get_field(&self.data, name)
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
    pub fn get_field_by_index<T: 'static + Clone>(&self, index: usize) -> T {
        self.type_layout.get_field_by_index(&self.data, index)
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
}

#[derive(Debug, Clone)]
pub struct DynamicTypeLayout {
    type_id: TypeId,
    size: usize,
    align: usize,
    default: unsafe fn() -> Vec<u8>,
    drop_fn: Option<fn(*const u8)>,
    name: &'static str,
}

impl DynamicTypeLayout {
    pub fn of<T: Any + Default + DefaultBytes>() -> Self {
        DynamicTypeLayout {
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
unsafe trait VecToType {
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
