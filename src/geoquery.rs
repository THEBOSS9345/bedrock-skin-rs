//! Reading a whole geometry file and picking values out of it by path.
//! See docs/geometry-format.md#picking-values-out-of-a-file.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::Error;
use crate::geometry::{
    Bone, Cube, Geometry, Locator, parse_geometry, read_bone, read_cube, read_f64s, read_locator,
};
use crate::jsonread::Reader;
use crate::polymesh::PolyMesh;

/// A whole geometry file, every field kept - including ones this library has
/// no type for - so any value in it can be picked out by a path.
/// [`parse_geometry`] gives the typed models the renderer uses; a tree is
/// for reading a file.
///
/// ```
/// # let raw = br#"{"minecraft:geometry":[{"description":{"identifier":"geometry.a"},"bones":[{"name":"rightArm","pivot":[-5,22,0]}]}]}"#;
/// let tree = bedrock_skin::parse_geometry_tree(raw)?;
/// let pivot = tree.get("geometry.a/bones/rightArm/pivot").unwrap();
/// assert_eq!(pivot.as_f64s(), Some(vec![-5.0, 22.0, 0.0]));
/// # Ok::<(), bedrock_skin::Error>(())
/// ```
///
/// Both of Bedrock's formats read into the same shape, the modern one: each
/// model is an object with a `description` (identifier, texture_width,
/// texture_height, visible_bounds_*) and `bones`, so one path works on
/// either.
#[derive(Clone, Debug)]
pub struct GeometryTree {
    /// The file's format_version, e.g. "1.12.0".
    pub format_version: String,
    models: Vec<(String, Value)>,
    raw: Vec<u8>,
}

/// One value a path picked out: its canonical path, with bones and other
/// named entries by name, and the value itself.
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryValue {
    pub path: String,
    pub value: Value,
}

/// Reads a geometry file of either format into a tree.
pub fn parse_geometry_tree(raw: &[u8]) -> Result<GeometryTree, Error> {
    let top: Value = serde_json::from_slice(raw).map_err(Error::Json)?;
    let top = match top {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        _ => return Err(Error::Geometry("the top level is not an object".into())),
    };
    let mut t = GeometryTree {
        format_version: top
            .get("format_version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        models: Vec::new(),
        raw: raw.to_vec(),
    };
    if let Some(Value::Array(list)) = top.get("minecraft:geometry") {
        for (i, m) in list.iter().enumerate() {
            let Value::Object(node) = m else {
                return Err(Error::Geometry(format!("model {i} is not an object")));
            };
            let id = node
                .get("description")
                .and_then(|d| d.get("identifier"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            t.models.push((id, m.clone()));
        }
    } else {
        // Legacy: each model is a top-level key. Its texture size and bounds
        // move into a description, so paths match the modern format.
        for (key, v) in &top {
            let Value::Object(node) = v else { continue };
            if key == "format_version" {
                continue;
            }
            if !node
                .get("bones")
                .and_then(Value::as_array)
                .is_some_and(|b| !b.is_empty())
            {
                continue; // as parse_geometry skips it
            }
            let mut desc = Map::new();
            desc.insert("identifier".into(), Value::String(key.clone()));
            let mut out = Map::new();
            for (k, val) in node {
                match k.as_str() {
                    "texturewidth" => {
                        desc.insert("texture_width".into(), val.clone());
                    }
                    "textureheight" => {
                        desc.insert("texture_height".into(), val.clone());
                    }
                    "visible_bounds_width" | "visible_bounds_height" | "visible_bounds_offset" => {
                        desc.insert(k.clone(), val.clone());
                    }
                    _ => {
                        out.insert(k.clone(), val.clone());
                    }
                }
            }
            out.insert("description".into(), Value::Object(desc));
            t.models.push((key.clone(), Value::Object(out)));
        }
        // The same order as parse_geometry.
        t.models.sort_by(|a, b| a.0.cmp(&b.0));
    }
    if t.models.is_empty() {
        return Err(Error::NoGeometryModels);
    }
    Ok(t)
}

impl GeometryTree {
    /// The models' identifiers, in the same order as [`parse_geometry`].
    pub fn identifiers(&self) -> Vec<&str> {
        self.models.iter().map(|(id, _)| id.as_str()).collect()
    }

    /// Every value the path picks out, in file order; empty when nothing
    /// matches.
    ///
    /// A path is segments separated by `/`. The first picks the model by
    /// identifier, the rest walk into it:
    ///
    /// - an object's field by name: `description`, `bones`, `locators`, `pivot`
    /// - an array element by index, from 0 (`-1` is the last), or - for a
    ///   list of named things, such as bones - by its name: `bones/rightArm`
    /// - `*` for every model, field or element at that level
    ///
    /// Names match exactly, else case-insensitively, as the game matches
    /// bones. A legacy model named `geometry.a:geometry.b` (one inheriting
    /// from another) is also picked by `geometry.a`. An empty path returns
    /// every model.
    pub fn select(&self, path: &str) -> Vec<GeometryValue> {
        let segs = split_path(path);
        let mut out = Vec::new();
        let models: Vec<&(String, Value)> = match segs.first() {
            None | Some(&"*") => self.models.iter().collect(),
            Some(first) => self.pick_models(first),
        };
        let rest = if segs.is_empty() { &[][..] } else { &segs[1..] };
        for (id, node) in models {
            walk(node, rest, id.clone(), &mut out);
        }
        out
    }

    /// The first value the path picks out.
    pub fn get(&self, path: &str) -> Option<GeometryValue> {
        self.select(path).into_iter().next()
    }

    /// The typed models: the same as [`parse_geometry`] on the file.
    pub fn geometries(&self) -> Result<Vec<Geometry>, Error> {
        parse_geometry(&self.raw)
    }

    fn pick_models(&self, seg: &str) -> Vec<&(String, Value)> {
        let matchers: [&dyn Fn(&str) -> bool; 3] =
            [&|id| id == seg, &|id| eq_fold(id, seg), &|id| {
                id.split_once(':')
                    .is_some_and(|(base, _)| eq_fold(base, seg))
            }];
        for m in matchers {
            let out: Vec<_> = self.models.iter().filter(|(id, _)| m(id)).collect();
            if !out.is_empty() {
                return out;
            }
        }
        Vec::new()
    }
}

fn eq_fold(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn split_path(path: &str) -> Vec<&str> {
    let p = path.trim().trim_matches('/');
    if p.is_empty() {
        Vec::new()
    } else {
        p.split('/').collect()
    }
}

/// Go's strconv.Atoi: an optionally signed decimal integer.
fn atoi(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn walk(v: &Value, segs: &[&str], path: String, out: &mut Vec<GeometryValue>) {
    let Some((&seg, rest)) = segs.split_first() else {
        out.push(GeometryValue {
            path,
            value: v.clone(),
        });
        return;
    };
    match v {
        Value::Object(node) => {
            if seg == "*" {
                let mut keys: Vec<&String> = node.keys().collect();
                keys.sort();
                for k in keys {
                    walk(&node[k], rest, format!("{path}/{k}"), out);
                }
                return;
            }
            if let Some(child) = node.get(seg) {
                walk(child, rest, format!("{path}/{seg}"), out);
                return;
            }
            let mut keys: Vec<&String> = node.keys().collect();
            keys.sort();
            if let Some(k) = keys.into_iter().find(|k| eq_fold(k, seg)) {
                walk(&node[k], rest, format!("{path}/{k}"), out);
            }
        }
        Value::Array(list) => {
            if seg == "*" {
                for (i, child) in list.iter().enumerate() {
                    walk(
                        child,
                        rest,
                        format!("{path}/{}", element_name(child, i)),
                        out,
                    );
                }
                return;
            }
            if let Some(mut i) = atoi(seg) {
                if i < 0 {
                    i += list.len() as i64;
                }
                if i >= 0 && (i as usize) < list.len() {
                    let child = &list[i as usize];
                    walk(
                        child,
                        rest,
                        format!("{path}/{}", element_name(child, i as usize)),
                        out,
                    );
                }
                return;
            }
            for fold in [false, true] {
                for (i, child) in list.iter().enumerate() {
                    let name = named_element(child);
                    if !name.is_empty() && (name == seg || fold && eq_fold(name, seg)) {
                        walk(
                            child,
                            rest,
                            format!("{path}/{}", element_name(child, i)),
                            out,
                        );
                        return;
                    }
                }
            }
        }
        _ => {}
    }
}

fn named_element(v: &Value) -> &str {
    v.get("name").and_then(Value::as_str).unwrap_or_default()
}

/// How an array element appears in a canonical path: its name when it has a
/// usable one (bones do), else its index.
fn element_name(v: &Value, i: usize) -> String {
    let name = named_element(v);
    if !name.is_empty() && !name.contains('/') && atoi(name).is_none() && name != "*" {
        return name.to_string();
    }
    i.to_string()
}

impl GeometryValue {
    /// The value as a number.
    pub fn as_f64(&self) -> Option<f64> {
        match &self.value {
            Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }

    /// The value as a list of numbers - a pivot, an origin, a size.
    pub fn as_f64s(&self) -> Option<Vec<f64>> {
        let list = self.value.as_array()?;
        list.iter().map(|e| e.as_f64()).collect()
    }

    /// The value as a string - a name, a parent, an identifier.
    pub fn as_str(&self) -> Option<&str> {
        self.value.as_str()
    }

    /// The value as a bone, read as [`parse_geometry`] reads one.
    pub fn bone(&self) -> Option<Bone> {
        let mut r = Reader::default();
        let mut b = Bone::default();
        read_bone(&mut r, &self.value, &mut b);
        (!r.type_error && self.value.is_object()).then_some(b)
    }

    /// The value as a cube.
    pub fn cube(&self) -> Option<Cube> {
        let mut r = Reader::default();
        let mut c = Cube::default();
        read_cube(&mut r, &self.value, &mut c);
        (!r.type_error && self.value.is_object()).then_some(c)
    }

    /// The value as a poly mesh, read as [`Bone::mesh`] reads one; e.g.
    /// `tree.get("*/bones/body/poly_mesh")`.
    pub fn poly_mesh(&self) -> Option<PolyMesh> {
        if !self.value.is_object() {
            return None;
        }
        Bone {
            poly_mesh: Some(self.value.clone()),
            ..Bone::default()
        }
        .mesh()
    }

    /// The value as a locator, in either of its forms.
    pub fn locator(&self) -> Option<Locator> {
        (self.value.is_object() || read_f64s(&self.value).is_some())
            .then(|| read_locator(&self.value))
    }

    /// The value decoded into any serde type.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.value.clone())
    }

    /// The value as JSON.
    pub fn json(&self) -> String {
        self.value.to_string()
    }
}
