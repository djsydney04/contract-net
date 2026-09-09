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
#[serde(tag = "type")]
pub(crate) enum Incoming {
    #[serde(rename = "REGISTERED")]
    Registered {
        name: String,
        #[serde(default)]
        rules: Rules,
        #[serde(default)]
        open_cfps: Vec<Task>,
    },
    #[serde(rename = "CFP")]
    Cfp {
        #[serde(flatten)]
        task: Task,
    },
    #[serde(rename = "ACCEPT_PROPOSAL")]
    AcceptProposal { task_id: u64 },
    #[serde(rename = "REJECT_PROPOSAL")]
    RejectProposal {
        task_id: u64,
        winner: Option<String>,
        winning_price: Option<f64>,
    },
    #[serde(rename = "SETTLED")]
    Settled {
        #[serde(flatten)]
        settlement: Settlement,
    },
    #[serde(rename = "BID_INVALID")]
    BidInvalid {
        task_id: u64,
        #[serde(default)]
        reason: String,
    },
    #[serde(rename = "ERROR")]
    Error { code: String, message: String },
    #[serde(other)]
    Unknown,
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
