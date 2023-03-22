use std::{env, path::PathBuf, process::Command, io::{Read, Write}, fs::File, collections::HashMap};

use wavefront_obj::obj::Primitive;

pub fn parse_obj() {
    let mut path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    path.push("assets");
    path.push("maxwell.obj");
    println!("cargo:rerun-if-changed={}", path.display());

    let obj_file = {
        let mut file = File::open(path).unwrap();
        let mut obj_file = String::new();
        file.read_to_string(&mut obj_file).unwrap();
        obj_file
    };
    let obj = wavefront_obj::obj::parse(&obj_file).unwrap();
    let mut result = String::from("struct MaxwellModel { pub vertices: &'static [f32],");

    let object = obj.objects.first().unwrap();

    for material in &object.geometry {
        if let Some(name) = &material.material_name {
            result.push_str("pub ");
            result.push_str(name);
            result.push_str(": &'static [u16],");
        }
    }
    result.push_str("} const MAXWELL_MODEL: MaxwellModel = MaxwellModel { vertices: &");

    let mut vertices = HashMap::new();
    let mut unrolled = vec![];
    for material in &object.geometry {
        for shape in &material.shapes {
            let indices = match &shape.primitive {
                Primitive::Point(a) => vec![a],
                Primitive::Line(a, b) => vec![a, b],
                Primitive::Triangle(a, b, c) => vec![a, b, c],
            };

            for index in indices {
                let key = (index.0, index.1.unwrap());
                if !vertices.contains_key(&key) {
                    let id = vertices.len();
                    let vertex = object.vertices[key.0];
                    let uv = object.tex_vertices[key.1];
                    unrolled.push(vertex.x as f32);
                    unrolled.push(vertex.y as f32);
                    unrolled.push(vertex.z as f32);
                    unrolled.push(uv.u as f32);
                    unrolled.push(uv.v as f32);

                    vertices.insert(key, id);
                }
            }
        }
    }

    result.push_str(&format!("{unrolled:?}"));
    result.push(',');

    for material in &object.geometry {
        if let Some(name) = &material.material_name {
            result.push_str(name);
            result.push_str(": &");
            let mut the_indices = vec![];
            for shape in &material.shapes {
                let indices = match &shape.primitive {
                    Primitive::Point(a) => vec![a],
                    Primitive::Line(a, b) => vec![a, b],
                    Primitive::Triangle(a, b, c) => vec![a, b, c],
                };

                for index in indices {
                    let key = (index.0, index.1.unwrap());
                    let id = *vertices.get(&key).unwrap();
                    the_indices.push(id);
                }
            }
            result.push_str(&format!("{the_indices:?},"));
        }
    }
    result.push_str("};");

    let mut file = File::create(PathBuf::from(env::var("OUT_DIR").unwrap()).join("maxwell.rs")).unwrap();
    file.write_all(result.as_bytes()).unwrap();
}


fn parse_texture(name: &str) {
    let mut path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    path.push("assets");
    println!("cargo:rerun-if-changed={}", path.join(format!("{name}.t3s")).display());
    println!("cargo:rerun-if-changed={}", path.join(format!("{name}.png")).display());

    let mut cmd = Command::new("tex3ds");
    cmd.arg("-i");
    cmd.arg(path.join(format!("{name}.t3s")));
    cmd.arg("-o");
    cmd.arg(PathBuf::from(env::var("OUT_DIR").unwrap()).join(format!("{name}.t3x")));
    let status = cmd.spawn().unwrap().wait().unwrap();
    if !status.success() {
        panic!("failed to parse texture");
    }
}

fn main() {
    let shader_path = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut shader_path = PathBuf::from(shader_path);
    shader_path.push("assets");
    shader_path.push("shader.v.pica");
    println!("cargo:rerun-if-changed={}", shader_path.display());

    let mut cmd = Command::new("picasso");
    cmd.arg(shader_path);
    cmd.arg("-o");
    cmd.arg(PathBuf::from(env::var("OUT_DIR").unwrap()).join("shader.shbin"));
    let status = cmd.spawn().unwrap().wait().unwrap();
    if !status.success() {
        panic!("failed to compile shader");
    }

    parse_texture("body");
    parse_texture("whiskers");

    parse_obj();
}
