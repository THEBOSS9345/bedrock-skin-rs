//! Reading JSON the way Go's encoding/json does, which is how the Go version
//! reads geometry: object keys match field names exactly or ignoring case, a
//! later key overwrites an earlier one, null leaves a number, string or bool
//! as it was and empties a list, unknown keys are ignored, and a value of the
//! wrong type is noted but does not stop the rest being read. Callers decide
//! what a noted type error means, as the Go code does with the error
//! json.Unmarshal returns.

use serde_json::Value;

/// Collects whether any value had the wrong type.
#[derive(Default)]
pub(crate) struct Reader {
    pub type_error: bool,
}

impl Reader {
    pub fn f64(&mut self, v: &Value, out: &mut f64) {
        match v {
            Value::Number(n) => match n.as_f64() {
                Some(f) => *out = f,
                None => self.type_error = true,
            },
            Value::Null => {}
            _ => self.type_error = true,
        }
    }

    pub fn opt_f64(&mut self, v: &Value, out: &mut Option<f64>) {
        match v {
            Value::Null => *out = None,
            _ => {
                let mut f = 0.0;
                let before = self.type_error;
                self.type_error = false;
                self.f64(v, &mut f);
                if !self.type_error {
                    *out = Some(f);
                }
                self.type_error |= before;
            }
        }
    }

    pub fn string(&mut self, v: &Value, out: &mut String) {
        match v {
            Value::String(s) => *out = s.clone(),
            Value::Null => {}
            _ => self.type_error = true,
        }
    }

    pub fn bool(&mut self, v: &Value, out: &mut bool) {
        match v {
            Value::Bool(b) => *out = *b,
            Value::Null => {}
            _ => self.type_error = true,
        }
    }

    pub fn f64s(&mut self, v: &Value, out: &mut Vec<f64>) {
        self.list(v, out, |r, item, f: &mut f64| r.f64(item, f));
    }

    /// A list: each element read into a fresh default, null clearing it.
    pub fn list<T: Default>(
        &mut self,
        v: &Value,
        out: &mut Vec<T>,
        mut read: impl FnMut(&mut Self, &Value, &mut T),
    ) {
        match v {
            Value::Array(items) => {
                out.clear();
                for item in items {
                    let mut t = T::default();
                    read(self, item, &mut t);
                    out.push(t);
                }
            }
            Value::Null => out.clear(),
            _ => self.type_error = true,
        }
    }

    /// An object's fields: each key, in order, passed to `field` with its
    /// name lower-cased for matching. Null leaves the target as it was.
    pub fn object(&mut self, v: &Value, mut field: impl FnMut(&mut Self, &str, &Value)) {
        match v {
            Value::Object(map) => {
                for (key, val) in map {
                    field(self, &key.to_lowercase(), val);
                }
            }
            Value::Null => {}
            _ => self.type_error = true,
        }
    }

    /// A map with string keys: entries keyed exactly, null clearing it.
    pub fn map<T: Default>(
        &mut self,
        v: &Value,
        out: &mut Vec<(String, T)>,
        mut read: impl FnMut(&mut Self, &Value, &mut T),
    ) {
        match v {
            Value::Object(map) => {
                for (key, val) in map {
                    let mut t = T::default();
                    read(self, val, &mut t);
                    if let Some(slot) = out.iter_mut().find(|(k, _)| k == key) {
                        slot.1 = t;
                    } else {
                        out.push((key.clone(), t));
                    }
                }
            }
            Value::Null => out.clear(),
            _ => self.type_error = true,
        }
    }
}
