use rand::{rngs::StdRng, Rng, SeedableRng};

#[derive(Clone)]
pub struct Table {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub numeric: Vec<bool>,
}

impl Table {
    pub fn parse(name: &str, text: &str) -> Result<Self, String> {
        Self::parse_limited(name, text, 5000, 65)
    }
    pub fn parse_research(name: &str, text: &str) -> Result<Self, String> {
        Self::parse_limited(name, text, 50000, 67)
    }
    fn parse_limited(
        name: &str,
        text: &str,
        max_rows: usize,
        max_columns: usize,
    ) -> Result<Self, String> {
        // RFC-style quoted fields, doubled quotes, embedded newlines, CRLF and BOM.
        let mut records = Vec::new();
        let mut record = Vec::new();
        let mut field = String::new();
        let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
        let mut quoted = false;
        let mut closed = false;
        while let Some(c) = chars.next() {
            if quoted {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        quoted = false;
                        closed = true;
                    }
                } else {
                    field.push(c);
                }
                continue;
            }
            match c {
                '"' if field.is_empty() && !closed => quoted = true,
                ',' => {
                    record.push(std::mem::take(&mut field));
                    closed = false;
                }
                '\n' | '\r' => {
                    if c == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    record.push(std::mem::take(&mut field));
                    closed = false;
                    if record.iter().any(|v| !v.trim().is_empty()) {
                        records.push(std::mem::take(&mut record));
                    } else {
                        record.clear();
                    }
                }
                ' ' | '\t' if closed => {}
                _ if closed => {
                    return Err("Unexpected characters after a closing CSV quote.".into())
                }
                '"' => {
                    return Err(
                        "A CSV quote must start a field; escape internal quotes by doubling them."
                            .into(),
                    )
                }
                _ => field.push(c),
            }
        }
        if quoted {
            return Err("Unclosed quoted CSV field.".into());
        }
        if !field.is_empty() || !record.is_empty() || closed {
            record.push(field);
            if record.iter().any(|v| !v.trim().is_empty()) {
                records.push(record);
            }
        }
        if records.len() < 5 {
            return Err("CSV needs a header and at least four data rows.".into());
        }
        let headers: Vec<String> = records
            .remove(0)
            .into_iter()
            .map(|s| s.trim().to_owned())
            .collect();
        if headers.len() < 2 || headers.len() > max_columns {
            return Err(format!(
                "Use 2–{max_columns} columns, including the response and metadata roles."
            ));
        }
        let mut unique = std::collections::HashSet::new();
        if headers
            .iter()
            .any(|h| h.is_empty() || !unique.insert(h.clone()))
        {
            return Err("Column names must be nonempty and unique.".into());
        }
        if records.len() > max_rows {
            return Err(format!("This workspace permits at most {max_rows} observations. Research supports 50,000 rows within its byte limit; native teaching supports 5,000."));
        }
        for (i, row) in records.iter().enumerate() {
            if row.len() != headers.len() {
                return Err(format!(
                    "Data row {} has {} fields; expected {}.",
                    i + 1,
                    row.len(),
                    headers.len()
                ));
            }
        }
        // Missing/nonfinite values are never silently removed or imputed.
        let numeric = (0..headers.len())
            .map(|j| {
                records.iter().all(|r| {
                    r[j].trim()
                        .parse::<f64>()
                        .is_ok_and(|v| v.is_finite() && v.abs() <= 1e12)
                })
            })
            .collect();
        Ok(Self {
            name: name.into(),
            headers,
            rows: records,
            numeric,
        })
    }

    pub fn dataset(&self, target: usize, features: &[usize]) -> Result<Data, String> {
        if features.is_empty() {
            return Err("Select at least one numeric predictor.".into());
        }
        if target >= self.headers.len()
            || !self.numeric[target]
            || features
                .iter()
                .any(|&j| j >= self.headers.len() || j == target || !self.numeric[j])
        {
            return Err("Select distinct finite numeric predictors and a numeric response. Missing values must be resolved first.".into());
        }
        Ok(Data {
            names: features.iter().map(|&j| self.headers[j].clone()).collect(),
            target: self.headers[target].clone(),
            x: self
                .rows
                .iter()
                .map(|r| {
                    features
                        .iter()
                        .map(|&j| r[j].trim().parse().unwrap())
                        .collect()
                })
                .collect(),
            y: self
                .rows
                .iter()
                .map(|r| r[target].trim().parse().unwrap())
                .collect(),
        })
    }

    pub fn demo(kind: usize) -> Self {
        let mut rng = StdRng::seed_from_u64(4710);
        let mut text = String::from("x1,x2,response\n");
        for i in 0..180 {
            let x = -3.0 + 6.0 * i as f64 / 179.0;
            let z: f64 = rng.gen_range(-3.0..3.0);
            let noise: f64 = rng.gen_range(-0.65..0.65);
            let y = if kind == 0 || kind == 2 {
                2.0 * (1.6 * x).sin() + 0.4 * x + noise * if kind == 2 { 4.0 } else { 1.0 }
            } else {
                0.8 * x + if z > 0.0 { 2.0 * x } else { -1.5 * x } + noise
            };
            text.push_str(&format!("{x:.8},{z:.8},{y:.8}\n"));
        }
        Self::parse(
            if kind == 0 {
                "Synthetic nonlinear signal"
            } else if kind == 2 {
                "Synthetic noisy signal"
            } else {
                "Synthetic interaction"
            },
            &text,
        )
        .unwrap()
    }
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Data {
    pub names: Vec<String>,
    pub target: String,
    pub x: Vec<Vec<f64>>,
    pub y: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn research_csv_limit_is_distinct_from_native_teaching() {
        let text = "x,y\n".to_owned()
            + (0..5001)
                .map(|i| format!("{i},{}\n", i + 1))
                .collect::<String>()
                .as_str();
        assert!(Table::parse("native", &text).is_err());
        assert_eq!(
            Table::parse_research("research", &text).unwrap().rows.len(),
            5001
        );
    }
    #[test]
    fn csv_quoted_newlines_bom_and_non_numeric() {
        let t = Table::parse("test", "\u{feff}x,label,y\r\n1,\"a,b\",2\r\n2,\"a\"\"b\",3\r\n3,\"line\nbreak\",4\r\n4,z,5\r\n").unwrap();
        assert_eq!(t.rows[1][1], "a\"b");
        assert_eq!(t.rows[2][1], "line\nbreak");
        assert_eq!(t.numeric, vec![true, false, true]);
        assert!(t.dataset(2, &[0]).is_ok());
        assert!(t.dataset(2, &[1]).is_err());
    }
    #[test]
    fn csv_rejects_bad_inputs() {
        for text in [
            "a,a\n1,2\n2,3\n3,4\n4,5",
            "a,b\n1,2,3\n2,3\n3,4\n4,5",
            "a,b\n1,\"bad",
        ] {
            assert!(Table::parse("bad", text).is_err());
        }
        let t = Table::parse("missing", "x,y\n1,2\n2,NaN\n3,4\n4,5").unwrap();
        assert!(!t.numeric[1]);
    }
}
