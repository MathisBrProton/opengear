//! Serde helpers shared by the configuration types.

/// (De)serialize a `BTreeMap<usize, V>` with **string** keys.
///
/// TOML tables (and JSON objects) only allow string keys: `toml` refuses to serialize integer
/// map keys (`KeyNotString`), which used to make the whole configuration unsavable as soon as a
/// per-key color or an ARGB LED count was set. Keys are written as `"12"` and parsed back to
/// `usize`; JSON produced by older exports (`{"12": ...}`) stays readable.
pub mod usize_key_map {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<V, S>(map: &BTreeMap<usize, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        V: Serialize,
        S: Serializer,
    {
        let as_strings: BTreeMap<String, &V> =
            map.iter().map(|(k, v)| (k.to_string(), v)).collect();
        as_strings.serialize(serializer)
    }

    pub fn deserialize<'de, V, D>(deserializer: D) -> Result<BTreeMap<usize, V>, D::Error>
    where
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let raw = BTreeMap::<String, V>::deserialize(deserializer)?;
        raw.into_iter()
            .map(|(k, v)| {
                k.trim()
                    .parse::<usize>()
                    .map(|idx| (idx, v))
                    .map_err(|_| D::Error::custom(format!("clé d'index invalide : {k:?}")))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Holder {
        #[serde(default, with = "super::usize_key_map")]
        map: BTreeMap<usize, u8>,
    }

    #[test]
    fn toml_round_trip_with_integer_keys() {
        let mut map = BTreeMap::new();
        map.insert(5, 1);
        map.insert(192, 2);
        let h = Holder { map };
        let s = toml::to_string_pretty(&h).expect("integer keys must serialize");
        let back: Holder = toml::from_str(&s).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn json_round_trip_and_legacy_string_keys() {
        let back: Holder = serde_json::from_str(r#"{"map":{"7":3}}"#).unwrap();
        assert_eq!(back.map.get(&7), Some(&3));
        let s = serde_json::to_string(&back).unwrap();
        assert_eq!(serde_json::from_str::<Holder>(&s).unwrap(), back);
    }

    #[test]
    fn rejects_non_numeric_keys() {
        assert!(toml::from_str::<Holder>("[map]\nabc = 1\n").is_err());
    }

    #[test]
    fn missing_map_defaults_to_empty() {
        let h: Holder = toml::from_str("").unwrap();
        assert!(h.map.is_empty());
    }
}
