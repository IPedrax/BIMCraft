//! Schedules' tables: CSV (`schedule::to_csv`) and a one-sheet workbook (`schedule::to_xlsx`).

use super::super::{Content, Ctx, Family, Format, Options, Out, Prepared, Takes};
use crate::schedule::Table;

/// The one table a table format writes, and its name.
fn table(items: &[Prepared]) -> Result<(&Table, &str), String> {
    match items {
        [item] => match &item.content {
            Content::Table(t) => Ok((t, item.title.as_str())),
            _ => Err(format!("{} has no table to write", item.title)),
        },
        _ => Err("a table file holds one schedule".to_string()),
    }
}

pub struct Csv;

impl Format for Csv {
    fn id(&self) -> &'static str {
        "csv"
    }
    fn label(&self) -> &'static str {
        "CSV"
    }
    fn extension(&self) -> &'static str {
        "csv"
    }
    fn family(&self) -> Family {
        Family::Schedule
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Table]
    }
    fn write(&self, items: &[Prepared], _: &Options, _: &Ctx) -> Result<Vec<Out>, String> {
        let (t, _) = table(items)?;
        Ok(vec![Out { suffix: None, bytes: crate::schedule::to_csv(t).into_bytes() }])
    }
}

pub struct Xlsx;

impl Format for Xlsx {
    fn id(&self) -> &'static str {
        "xlsx"
    }
    fn label(&self) -> &'static str {
        "XLSX"
    }
    fn extension(&self) -> &'static str {
        "xlsx"
    }
    fn family(&self) -> Family {
        Family::Schedule
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Table]
    }
    fn write(&self, items: &[Prepared], _: &Options, _: &Ctx) -> Result<Vec<Out>, String> {
        let (t, name) = table(items)?;
        Ok(vec![Out { suffix: None, bytes: crate::schedule::to_xlsx(t, name) }])
    }
}
