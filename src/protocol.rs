use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Task {
    pub task_id: u64,
    pub task_type: String,
    #[serde(default)]
    pub params: Value,
    pub budget: f64,
    pub deadline_s: f64,
    #[serde(default)]
    pub bid_window_ms: u64,
    #[serde(default = "first_attempt")]
    pub attempt: u64,
}

fn first_attempt() -> u64 {
    1
}

impl Task {
    pub fn work(&self) -> anyhow::Result<f64> {
        crate::benchmark::work_units(&self.task_type, &self.params)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bid {
    pub price: f64,
    pub est_seconds: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub award_policy: String,
    pub time_weight: f64,
    pub cost_rate: f64,
    pub penalty_rate: f64,
    pub late_credit: f64,
    pub concurrency: usize,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            award_policy: "best_value".into(),
            time_weight: 2.0,
            cost_rate: 1.0,
            penalty_rate: 0.5,
            late_credit: 0.0,
            concurrency: 1,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Settlement {
    pub task_id: u64,
    #[serde(default)]
    pub task_type: String,
    pub verdict: String,
    pub revenue: f64,
    pub cost: f64,
    pub penalty: f64,
    pub profit: f64,
    pub runtime: Option<f64>,
    pub est_seconds: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Registration {
    pub name: String,
    #[serde(default)]
    pub rules: Rules,
    #[serde(default)]
    pub open_cfps: Vec<Task>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Award {
    pub task_id: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Rejection {
    pub task_id: u64,
    pub winner: Option<String>,
    pub winning_price: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InvalidBid {
    pub task_id: u64,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ManagerError {
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
pub(crate) enum Incoming {
    Registered(Registration),
    Cfp(Task),
    AcceptProposal(Award),
    RejectProposal(Rejection),
    Settled(Settlement),
    BidInvalid(InvalidBid),
    Error(ManagerError),
    Unknown,
}

impl Incoming {
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(raw).context("invalid manager JSON")?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .context("manager message must have a string `type` field")?
            .to_owned();
        // Decode each payload directly through serde_json. Serde's internally
        // tagged enums and flatten buffer numbers in a format that cannot decode
        // decimal f64 fields with arbitrary_precision enabled. A JSON Value
        // retains both decimal fields and exact, arbitrarily large task integers.
        match kind.as_str() {
            "REGISTERED" => serde_json::from_value(value).map(Self::Registered),
            "CFP" => serde_json::from_value(value).map(Self::Cfp),
            "ACCEPT_PROPOSAL" => serde_json::from_value(value).map(Self::AcceptProposal),
            "REJECT_PROPOSAL" => serde_json::from_value(value).map(Self::RejectProposal),
            "SETTLED" => serde_json::from_value(value).map(Self::Settled),
            "BID_INVALID" => serde_json::from_value(value).map(Self::BidInvalid),
            "ERROR" => serde_json::from_value(value).map(Self::Error),
            _ => return Ok(Self::Unknown),
        }
        .with_context(|| format!("invalid {kind} message"))
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(crate) enum Outgoing {
    #[serde(rename = "REGISTER")]
    Register {
        name: String,
        token: Option<String>,
        machine: String,
        benchmark: Option<f64>,
    },
    #[serde(rename = "PROPOSE")]
    Propose {
        task_id: u64,
        price: f64,
        est_seconds: f64,
    },
    #[serde(rename = "REFUSE")]
    Refuse { task_id: u64 },
    #[serde(rename = "INFORM")]
    Inform {
        task_id: u64,
        result: String,
        runtime: f64,
    },
    #[serde(rename = "FAILURE")]
    Failure { task_id: u64, reason: String },
}

impl Outgoing {
    pub(crate) fn task_id(&self) -> Option<u64> {
        match self {
            Self::Register { .. } => None,
            Self::Propose { task_id, .. }
            | Self::Refuse { task_id }
            | Self::Inform { task_id, .. }
            | Self::Failure { task_id, .. } => Some(*task_id),
        }
    }
}
