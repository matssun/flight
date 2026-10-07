// SPDX-License-Identifier: MIT

use crate::validate::non_empty;
use crate::{Reject, RoleCode, Validate};

/// Join the fleet. The identity being enrolled is the client certificate this request
/// arrived under; the token only authorizes the join.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct EnrollRequest {
    #[prost(string, tag = "1")]
    pub token: String,
    #[prost(string, tag = "2")]
    pub display_name: String,
    #[prost(enumeration = "RoleCode", tag = "3")]
    pub role: i32,
}

impl Validate for EnrollRequest {
    fn validate(&self) -> Result<(), Reject> {
        non_empty(&self.token, "enroll_request.token")?;
        RoleCode::decode(self.role, "enroll_request.role").map(|_| ())
    }
}

/// Enrollment succeeded: the identity the orchestrator now trusts, and its own.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct EnrollResponse {
    #[prost(string, tag = "1")]
    pub node_id: String,
    #[prost(string, tag = "2")]
    pub orchestrator_id: String,
}

impl Validate for EnrollResponse {
    fn validate(&self) -> Result<(), Reject> {
        non_empty(&self.node_id, "enroll_response.node_id")?;
        non_empty(&self.orchestrator_id, "enroll_response.orchestrator_id")
    }
}
