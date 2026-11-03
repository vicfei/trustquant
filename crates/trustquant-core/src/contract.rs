//! External contract types (ADR-0001): schema version, request/response
//! envelope, 10-class error taxonomy (v1's 7 + provider/artifact/replay
//! unavailable), per-metric unit semantics, tolerance with scale anchors.

use serde_json::Value;

pub const SCHEMA_VERSION: &str = "0.2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    InvalidInput,
    UnsupportedFeature,
    InvalidMarketData,
    NoConvergence,
    NumericalFailure,
    ResourceLimit,
    InternalError,
    ProviderUnavailable,
    ArtifactUnavailable,
    ReplayUnavailable,
}

impl ErrorType {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorType::InvalidInput => "invalid_input",
            ErrorType::UnsupportedFeature => "unsupported_feature",
            ErrorType::InvalidMarketData => "invalid_market_data",
            ErrorType::NoConvergence => "no_convergence",
            ErrorType::NumericalFailure => "numerical_failure",
            ErrorType::ResourceLimit => "resource_limit",
            ErrorType::InternalError => "internal_error",
            ErrorType::ProviderUnavailable => "provider_unavailable",
            ErrorType::ArtifactUnavailable => "artifact_unavailable",
            ErrorType::ReplayUnavailable => "replay_unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ErrorEnvelope {
    pub error_type: ErrorType,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
    pub correctable: bool,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl ErrorEnvelope {
    pub fn invalid_input(message: String, field_path: &str) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::InvalidInput,
            message,
            field_path: Some(field_path.to_string()),
            correctable: true,
            retryable: false,
            detail: None,
        }
    }

    pub fn unsupported_feature(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::UnsupportedFeature,
            message,
            field_path: None,
            correctable: false,
            retryable: false,
            detail: None,
        }
    }

    pub fn numerical_failure(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::NumericalFailure,
            message,
            field_path: None,
            correctable: false,
            retryable: false,
            detail: None,
        }
    }

    pub fn internal(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::InternalError,
            message,
            field_path: None,
            correctable: false,
            retryable: true,
            detail: None,
        }
    }

    pub fn provider_unavailable(message: String, install_hint: &str) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::ProviderUnavailable,
            message,
            field_path: None,
            correctable: true,
            retryable: true,
            detail: Some(Value::String(install_hint.to_string())),
        }
    }

    pub fn artifact_unavailable(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::ArtifactUnavailable,
            message,
            field_path: None,
            correctable: false,
            retryable: false,
            detail: None,
        }
    }

    pub fn replay_unavailable(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::ReplayUnavailable,
            message,
            field_path: None,
            correctable: false,
            retryable: false,
            detail: None,
        }
    }

    pub fn invalid_market_data(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::InvalidMarketData,
            message,
            field_path: None,
            correctable: false,
            retryable: false,
            detail: None,
        }
    }

    pub fn no_convergence(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::NoConvergence,
            message,
            field_path: None,
            correctable: true,
            retryable: true,
            detail: None,
        }
    }

    pub fn resource_limit(message: String) -> Self {
        ErrorEnvelope {
            error_type: ErrorType::ResourceLimit,
            message,
            field_path: None,
            correctable: false,
            retryable: true,
            detail: None,
        }
    }

    pub fn error_type_str(&self) -> &'static str {
        self.error_type.as_str()
    }
}

/// Machine-readable unit semantics, per metric (ADR-0001 §4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Currency,
    Time,
    Dimensionless,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// Notional units of the underlying spot price.
    UnitsOfSpot,
    Years,
    /// Sensitivity per 1.00 unit of the underlying.
    PerUnitSpot,
    PerUnitSpotSquared,
    /// Sensitivity per 1.00 (i.e. 100 percentage points) of volatility.
    PerUnitVol,
    Unity,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnitSemantics {
    pub dimension: Dimension,
    pub scale: Scale,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_notional: Option<f64>,
}

impl UnitSemantics {
    pub fn units_of_spot() -> Self {
        UnitSemantics {
            dimension: Dimension::Currency,
            scale: Scale::UnitsOfSpot,
            convention: None,
            reference_notional: None,
        }
    }
    pub fn per_unit_spot() -> Self {
        UnitSemantics {
            dimension: Dimension::Dimensionless,
            scale: Scale::PerUnitSpot,
            convention: None,
            reference_notional: None,
        }
    }
    pub fn per_unit_spot_squared() -> Self {
        UnitSemantics {
            dimension: Dimension::Dimensionless,
            scale: Scale::PerUnitSpotSquared,
            convention: None,
            reference_notional: None,
        }
    }
    pub fn per_unit_vol() -> Self {
        UnitSemantics {
            dimension: Dimension::Dimensionless,
            scale: Scale::PerUnitVol,
            convention: Some("per 1.00 volatility (not per 1%)".into()),
            reference_notional: None,
        }
    }
    pub fn per_year() -> Self {
        UnitSemantics {
            dimension: Dimension::Currency,
            scale: Scale::Unity,
            convention: Some("per year".into()),
            reference_notional: None,
        }
    }
    pub fn years() -> Self {
        UnitSemantics {
            dimension: Dimension::Time,
            scale: Scale::Years,
            convention: None,
            reference_notional: None,
        }
    }
}

/// Tolerance with a scale anchor; anchor granularity = metric x scale type.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleType {
    ExpectedMagnitude,
    Notional,
    Annuity,
    UnitPrice,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NearZeroPolicy {
    pub threshold: f64,
    pub atol_floor: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToleranceSpec {
    pub atol: f64,
    pub rtol: f64,
    pub scale_type: ScaleType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub near_zero_policy: Option<NearZeroPolicy>,
}

impl ToleranceSpec {
    pub fn check(&self, actual: f64, expected: f64) -> Result<(), String> {
        if !actual.is_finite() || !expected.is_finite() {
            return Err(format!(
                "non-finite value: actual={actual}, expected={expected}"
            ));
        }
        let tol = match self.near_zero_policy {
            Some(nz) if expected.abs() <= nz.threshold => {
                nz.atol_floor + self.rtol * expected.abs()
            }
            _ => self.atol + self.rtol * expected.abs(),
        };
        let diff = (actual - expected).abs();
        if diff <= tol {
            Ok(())
        } else {
            Err(format!(
                "|{actual} - {expected}| = {diff} > tolerance {tol} (atol={}, rtol={}, scale_type={:?})",
                self.atol, self.rtol, self.scale_type
            ))
        }
    }
}

/// Request envelope (abstract contract; transports embed it).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequestEnvelope {
    pub schema_version: String,
    pub tool: String,
    pub params: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl RequestEnvelope {
    /// Major version must match the server's major (ADR-0001 §5).
    pub fn validate_schema_version(&self) -> Result<(), ErrorEnvelope> {
        let ok = self
            .schema_version
            .split('.')
            .next()
            .zip(SCHEMA_VERSION.split('.').next())
            .is_some_and(|(a, b)| a == b);
        if ok {
            Ok(())
        } else {
            Err(ErrorEnvelope {
                error_type: ErrorType::InvalidInput,
                message: format!(
                    "schema_version '{}' not supported; server supports major '{}'",
                    self.schema_version, SCHEMA_VERSION
                ),
                field_path: Some("schema_version".to_string()),
                correctable: false,
                retryable: false,
                detail: None,
            })
        }
    }
}

/// Core-level tool output; the Runtime wraps it with calculation_id /
/// manifest_hash and records the serving provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub engine_id: &'static str,
    pub provider_id: &'static str,
    pub result: Value,
    pub warnings: Vec<String>,
}
