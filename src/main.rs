#![feature(vec_into_raw_parts)]

use std::{
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use smartstring::alias::String;
use dynamic_types::*;

mod dynamic_types;

fn main() {
    let type_registry = TypeRegistry::default();

    let type_layout = DynamicTypeLayout::new(
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

pub fn kitype_to_dyn_type_layout(ctype: &str) -> StaticTypeLayout {
    if ctype.starts_with("class SharedPointer") {
        //Shared pointers aka Arcs
        let ctype = ctype
            .trim_start_matches("class SharedPointer<")
            .trim_end_matches('>');
        match ctype {
            "unsigned char" => StaticTypeLayout::of::<Option<Arc<u8>>>(),
            "char" => StaticTypeLayout::of::<Option<Arc<i8>>>(),
            "short" => StaticTypeLayout::of::<Option<Arc<i16>>>(),
            "unsigned short" => StaticTypeLayout::of::<Option<Arc<u16>>>(),
            "int" => StaticTypeLayout::of::<Option<Arc<i32>>>(),
            "unsigned int" => StaticTypeLayout::of::<Option<Arc<u32>>>(),
            "long" => StaticTypeLayout::of::<Option<Arc<i32>>>(),
            "unsigned long" => StaticTypeLayout::of::<Option<Arc<u32>>>(),
            "gid" => StaticTypeLayout::of::<Option<Arc<GID>>>(),
            "float" => StaticTypeLayout::of::<Option<Arc<f32>>>(),
            "double" => StaticTypeLayout::of::<Option<Arc<f64>>>(),
            "std::string" => StaticTypeLayout::of::<Option<Arc<String>>>(),
            "std::wstring" => StaticTypeLayout::of::<Option<Arc<String>>>(),
            "class Vector3D" => StaticTypeLayout::of::<Option<Arc<Vector3D>>>(),
            "class Color" => StaticTypeLayout::of::<Option<Arc<Color>>>(),
            "class Point" => StaticTypeLayout::of::<Option<Arc<Point>>>(),
            _ => panic!("Unhandled type: {}", ctype),
        }
    } else if ctype.ends_with('*') {
        //Raw pointers
        let ctype = ctype.trim_end_matches('*');
        match ctype {
            "unsigned char" => StaticTypeLayout::of::<Option<Box<u8>>>(),
            "char" => StaticTypeLayout::of::<Option<Box<i8>>>(),
            "short" => StaticTypeLayout::of::<Option<Box<i16>>>(),
            "unsigned short" => StaticTypeLayout::of::<Option<Box<u16>>>(),
            "int" => StaticTypeLayout::of::<Option<Box<i32>>>(),
            "unsigned int" => StaticTypeLayout::of::<Option<Box<u32>>>(),
            "long" => StaticTypeLayout::of::<Option<Box<i32>>>(),
            "unsigned long" => StaticTypeLayout::of::<Option<Box<u32>>>(),
            "gid" => StaticTypeLayout::of::<Option<Box<GID>>>(),
            "float" => StaticTypeLayout::of::<Option<Box<f32>>>(),
            "double" => StaticTypeLayout::of::<Option<Box<f64>>>(),
            "std::string" => StaticTypeLayout::of::<Option<Box<String>>>(),
            "std::wstring" => StaticTypeLayout::of::<Option<Box<String>>>(),
            "class Vector3D" => StaticTypeLayout::of::<Option<Box<Vector3D>>>(),
            "class Color" => StaticTypeLayout::of::<Option<Box<Color>>>(),
            "class Point" => StaticTypeLayout::of::<Option<Box<Point>>>(),
            _ => panic!("Unhandled type: {}", ctype),
        }
    } else {
        match ctype {
            //Value types
            "unsigned char" => StaticTypeLayout::of::<u8>(),
            "char" => StaticTypeLayout::of::<i8>(),
            "short" => StaticTypeLayout::of::<i16>(),
            "unsigned short" => StaticTypeLayout::of::<u16>(),
            "int" => StaticTypeLayout::of::<i32>(),
            "unsigned int" => StaticTypeLayout::of::<u32>(),
            "long" => StaticTypeLayout::of::<i32>(),
            "unsigned long" => StaticTypeLayout::of::<u32>(),
            "gid" => StaticTypeLayout::of::<GID>(),
            "float" => StaticTypeLayout::of::<f32>(),
            "double" => StaticTypeLayout::of::<f64>(),
            "std::string" => StaticTypeLayout::of::<String>(),
            "std::wstring" => StaticTypeLayout::of::<String>(),
            "class Vector3D" => StaticTypeLayout::of::<Vector3D>(),
            "class Color" => StaticTypeLayout::of::<Color>(),
            "class Point" => StaticTypeLayout::of::<Point>(),
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