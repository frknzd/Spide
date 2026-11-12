//! PlantUML export (filled in later).
use crate::metrics::Metrics;
use crate::model::*;
use std::collections::BTreeMap;

pub fn export(_view: &str, _d: &Design, _m: &Metrics, _layout: &BTreeMap<String, BTreeMap<String, Pos>>) -> Result<String, String> {
    Err("not implemented yet".into())
}
