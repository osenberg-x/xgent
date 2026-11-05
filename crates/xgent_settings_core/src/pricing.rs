//! 模型价格与成本计算（F-12）。
//!
//! 价格单位：USD / 1M tokens；0 或非法值视为未设置。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 全局价格配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PricingConfig {
    /// 默认输入单价（USD / 1M tokens）；0 = 未设置。
    #[serde(default)]
    pub input_per_million: f64,
    /// 默认输出单价（USD / 1M tokens）；0 = 未设置。
    #[serde(default)]
    pub output_per_million: f64,
    /// 按模型覆盖。
    #[serde(default)]
    pub models: BTreeMap<String, ModelPrice>,
}

/// 单个模型的价格覆盖。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelPrice {
    /// 输入单价；0 = 回落到默认价。
    #[serde(default)]
    pub input_per_million: f64,
    /// 输出单价；0 = 回落到默认价。
    #[serde(default)]
    pub output_per_million: f64,
}

impl PricingConfig {
    /// 解析某模型的生效价格；输入/输出任一未定价则返回 `None`。
    pub fn price_for(&self, model: &str) -> Option<(f64, f64)> {
        let model_price = self.models.get(model);
        let input = model_price
            .and_then(|price| positive_price(price.input_per_million))
            .or_else(|| positive_price(self.input_per_million))?;
        let output = model_price
            .and_then(|price| positive_price(price.output_per_million))
            .or_else(|| positive_price(self.output_per_million))?;
        Some((input, output))
    }

    /// 计算成本；未定价返回 `None`。
    pub fn cost(&self, prompt: u64, completion: u64, model: &str) -> Option<f64> {
        let (input, output) = self.price_for(model)?;
        let cost = prompt as f64 / 1_000_000.0 * input + completion as f64 / 1_000_000.0 * output;
        if cost.is_finite() && cost >= 0.0 {
            Some(cost)
        } else {
            None
        }
    }

    /// 价格是否可用（有限、非负）。
    pub fn valid_price(value: f64) -> bool {
        value.is_finite() && value >= 0.0
    }
}

fn positive_price(value: f64) -> Option<f64> {
    (PricingConfig::valid_price(value) && value > 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> PricingConfig {
        PricingConfig {
            input_per_million: 2.5,
            output_per_million: 10.0,
            models: BTreeMap::from([
                (
                    "gpt-4o-mini".to_string(),
                    ModelPrice {
                        input_per_million: 0.15,
                        output_per_million: 0.6,
                    },
                ),
                (
                    "partial".to_string(),
                    ModelPrice {
                        input_per_million: 1.0,
                        output_per_million: 0.0,
                    },
                ),
            ]),
        }
    }

    #[test]
    fn default_and_model_override() {
        let cfg = config();
        assert_eq!(cfg.price_for("gpt-4o"), Some((2.5, 10.0)));
        assert_eq!(cfg.price_for("gpt-4o-mini"), Some((0.15, 0.6)));
        assert_eq!(cfg.price_for("partial"), Some((1.0, 10.0)));
    }

    #[test]
    fn unpriced_and_invalid_prices() {
        let cfg = PricingConfig::default();
        assert_eq!(cfg.price_for("any"), None);
        assert_eq!(cfg.cost(100, 100, "any"), None);

        let invalid = PricingConfig {
            input_per_million: f64::NAN,
            output_per_million: -1.0,
            ..Default::default()
        };
        assert_eq!(invalid.price_for("any"), None);
    }

    #[test]
    fn cost_formula_and_zero_tokens() {
        let cfg = config();
        let cost = cfg.cost(1_000_000, 1_000_000, "gpt-4o").expect("计价");
        assert!((cost - 12.5).abs() < 1e-9);
        let small = cfg.cost(1, 1, "gpt-4o-mini").expect("计价");
        assert!(small > 0.0 && small.is_finite());
        assert_eq!(cfg.cost(0, 0, "gpt-4o"), Some(0.0));
    }

    #[test]
    fn toml_example_parses() {
        let global: super::super::GlobalConfig = toml::from_str(
            r#"
[pricing]
input_per_million = 2.5
output_per_million = 10.0

[pricing.models."gpt-4o-mini"]
input_per_million = 0.15
output_per_million = 0.6
"#,
        )
        .expect("解析");
        assert_eq!(global.pricing.price_for("gpt-4o"), Some((2.5, 10.0)));
        assert_eq!(global.pricing.price_for("gpt-4o-mini"), Some((0.15, 0.6)));
    }

    #[test]
    fn old_config_without_pricing_uses_default() {
        let global: super::super::GlobalConfig =
            toml::from_str("[preferences]\nlanguage = \"zh-CN\"\n").expect("解析");
        assert_eq!(global.pricing.price_for("any"), None);
    }
}
