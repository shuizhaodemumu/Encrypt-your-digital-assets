use crate::{core::*, platform};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Options {
    pub days: i32,
    pub value: u64,
    pub unit: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            days: 30,
            value: 60,
            unit: 0,
        }
    }
}
impl Options {
    pub fn parse(days: &str, value: &str, unit: usize) -> Result<Self> {
        let days = days.trim();
        let value = value.trim();
        ensure!(
            !days.is_empty() && days.bytes().all(|b| b.is_ascii_digit()),
            crate::i18n::text("授权天数请输入正整数")
        );
        ensure!(
            !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
            crate::i18n::text("首次导入窗口请输入正整数")
        );
        let result = Self {
            days: days.parse().context(crate::i18n::text("天数超出范围"))?,
            value: value
                .parse()
                .context(crate::i18n::text("首次导入窗口数值过大"))?,
            unit,
        };
        result.validate(now())?;
        Ok(result)
    }
    pub fn seconds(&self) -> Result<i64> {
        let factor = match self.unit {
            0 => 1,
            1 => 60,
            2 => 3600,
            3 => 86400,
            _ => anyhow::bail!(crate::i18n::text("请选择时间单位")),
        };
        let seconds = self
            .value
            .checked_mul(factor)
            .and_then(|n| i64::try_from(n).ok())
            .context(crate::i18n::text("首次导入窗口数值过大"))?;
        ensure!(seconds > 0, crate::i18n::text("首次导入窗口必须大于零"));
        Ok(seconds)
    }
    pub fn validate(&self, time: i64) -> Result<i64> {
        expiry(time, self.days)?;
        let seconds = self.seconds()?;
        let deadline = activation_deadline(time, seconds)?;
        expiry(deadline - 1, self.days)?;
        Ok(seconds)
    }
    pub fn load() -> Self {
        std::fs::read(platform::root().join("issuer-settings.json"))
            .ok()
            .filter(|b| b.len() <= 4096)
            .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
            .filter(|v| v.validate(now()).is_ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<()> {
        self.validate(now())?;
        atomic(
            &platform::root().join("issuer-settings.json"),
            &serde_json::to_vec(self)?,
        )
    }
    pub fn unit_caption(&self) -> &'static str {
        crate::i18n::text(match self.unit {
            0 => "秒",
            1 => "分钟",
            2 => "小时",
            _ => "天",
        })
    }
    pub fn summary(&self) -> String {
        crate::tr_format!(
            "首次导入：签发后 {} {}内。\n使用期限：首次激活成功后 {} 天。",
            "First import: within {} {} after issue.\nUsage: {} days from first activation.",
            self.value,
            self.unit_caption(),
            self.days
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn units_and_defaults() {
        assert_eq!(Options::default().seconds().unwrap(), 60);
        for (unit, factor) in [(0, 1), (1, 60), (2, 3600), (3, 86400)] {
            assert_eq!(
                Options::parse("30", "2", unit).unwrap().seconds().unwrap(),
                2 * factor
            );
        }
    }
    #[test]
    fn rejects_empty_zero_negative_fraction_invalid_units_and_overflow() {
        for v in ["", "0", "-1", "1.5", "abc", "18446744073709551616"] {
            assert!(Options::parse("30", v, 0).is_err(), "{v}");
        }
        for v in ["", "0", "-1", "x"] {
            assert!(Options::parse(v, "60", 0).is_err());
        }
        assert!(Options::parse("30", "60", 4).is_err());
        assert!(Options::parse("30", "18446744073709551615", 3).is_err());
        assert!(Options::parse("30", "999999999999", 0).is_err());
    }
    #[test]
    fn preference_roundtrip_validated() {
        let v = Options::parse("17", "90", 1).unwrap();
        let decoded: Options = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(v, decoded);
        assert_eq!(decoded.seconds().unwrap(), 5400);
    }
}
