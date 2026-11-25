//! A command's parameters, checked against its spec before the command runs.

use serde_json::{Map, Value};

use crate::{CommandSpec, Failure, Kind, fail};

pub struct Args {
    raw: Value,
}

impl Args {
    /// Refuses a call that leaves out a required parameter, passes one the command does not
    /// have, or passes the wrong kind of value. An agent that misspells a parameter is told so
    /// instead of having it silently ignored.
    pub fn check(spec: &CommandSpec, params: &Value) -> Result<Args, Failure> {
        let empty = Map::new();
        let map = match params {
            Value::Object(map) => map,
            Value::Null => &empty,
            _ => return fail(format!("{} takes its parameters as a JSON object", spec.name)),
        };
        for key in map.keys() {
            if !spec.params.iter().any(|p| p.name == key) {
                let known: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
                return fail(format!("{} has no parameter called {key}. It takes: {}", spec.name, known.join(", ")));
            }
        }
        for p in spec.params {
            match map.get(p.name) {
                None | Some(Value::Null) if p.required => return fail(format!("{} needs {}: {}", spec.name, p.name, p.description)),
                None | Some(Value::Null) => {}
                Some(value) if !fits(p.kind, value) => return fail(format!("{}: {} must be {}", spec.name, p.name, kind_words(p.kind))),
                Some(_) => {}
            }
        }
        Ok(Args { raw: Value::Object(map.clone()) })
    }

    /// The parameters as they were passed, for the journal.
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    fn get(&self, name: &str) -> Option<&Value> {
        self.raw.get(name).filter(|v| !v.is_null())
    }

    pub fn str(&self, name: &str) -> Result<String, Failure> {
        self.opt_str(name)?.map_or_else(|| fail(format!("{name} is required")), Ok)
    }

    pub fn opt_str(&self, name: &str) -> Result<Option<String>, Failure> {
        Ok(self.get(name).and_then(Value::as_str).map(str::to_owned))
    }

    pub fn int(&self, name: &str) -> Result<i64, Failure> {
        self.opt_int(name)?.map_or_else(|| fail(format!("{name} is required")), Ok)
    }

    pub fn opt_int(&self, name: &str) -> Result<Option<i64>, Failure> {
        Ok(self.get(name).and_then(Value::as_i64))
    }

    /// A number parameter, whole or not.
    pub fn opt_num(&self, name: &str) -> Result<Option<f64>, Failure> {
        match self.get(name) {
            None => Ok(None),
            Some(value) => match value.as_f64() {
                Some(number) if number.is_finite() => Ok(Some(number)),
                _ => fail(format!("{name} must be a number")),
            },
        }
    }

    pub fn opt_bool(&self, name: &str) -> Result<Option<bool>, Failure> {
        Ok(self.get(name).and_then(Value::as_bool))
    }

    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// A list-of-text parameter.
    pub fn strings(&self, name: &str) -> Vec<String> {
        self.get(name).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
    }

    /// A list-of-objects parameter.
    pub fn objects(&self, name: &str) -> Vec<Map<String, Value>> {
        self.get(name).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_object).cloned().collect()).unwrap_or_default()
    }

    /// A `{"key": "value"}` parameter as pairs, in the order given.
    pub fn text_map(&self, name: &str) -> Vec<(String, String)> {
        let Some(Value::Object(map)) = self.get(name) else {
            return Vec::new();
        };
        map.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect()
    }
}

fn fits(kind: Kind, value: &Value) -> bool {
    match kind {
        Kind::String => value.is_string(),
        Kind::Integer => value.is_i64(),
        Kind::Number => value.is_number(),
        Kind::Boolean => value.is_boolean(),
        Kind::TextMap => value.as_object().is_some_and(|m| m.values().all(Value::is_string)),
        Kind::Objects => value.as_array().is_some_and(|a| a.iter().all(Value::is_object)),
        Kind::Strings => value.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
    }
}

fn kind_words(kind: Kind) -> &'static str {
    match kind {
        Kind::String => "text",
        Kind::Integer => "a whole number",
        Kind::Number => "a number",
        Kind::Boolean => "true or false",
        Kind::TextMap => "an object of text values",
        Kind::Objects => "a list of objects",
        Kind::Strings => "a list of text values",
    }
}
