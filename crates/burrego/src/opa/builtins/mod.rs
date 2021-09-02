use anyhow::Result;
use std::collections::HashMap;

pub mod debugging;
pub mod encoding;
pub mod regex;
pub mod semver;
pub mod strings;

pub type BuiltinFunctionsMap =
    HashMap<&'static str, fn(&[serde_json::Value]) -> Result<serde_json::Value>>;

pub fn get_builtins() -> BuiltinFunctionsMap {
    let mut functions: BuiltinFunctionsMap = HashMap::new();

    // debugging
    functions.insert("trace", debugging::trace);

    // encoding
    functions.insert(
        "base64url.encode_no_pad",
        encoding::base64url::encode_no_pad,
    );
    functions.insert("urlquery.encode", encoding::urlquery::encode);
    functions.insert("urlquery.decode", encoding::urlquery::decode);
    functions.insert("urlquery.encode_object", encoding::urlquery::encode_object);
    functions.insert("urlquery.decode_object", encoding::urlquery::decode_object);
    functions.insert("json.is_valid", encoding::json::is_valid);
    functions.insert("yaml.marshal", encoding::yaml::marshal);
    functions.insert("yaml.unmarshal", encoding::yaml::unmarshal);
    functions.insert("hex.encode", encoding::hex::encode);
    functions.insert("hex.decode", encoding::hex::decode);

    // regex
    functions.insert("regex.split", regex::split);
    functions.insert("regex.template_match", regex::template_match);
    functions.insert("regex.find_n", regex::find_n);

    // semver
    functions.insert("semver.is_valid", semver::is_valid);
    functions.insert("semver.compare", semver::compare);

    // strings
    functions.insert("sprintf", strings::sprintf);

    functions
}
