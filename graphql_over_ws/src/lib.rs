use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use juniper::{ScalarRefValue, InputValue, ScalarValue};
use std::fmt;

pub trait GraphQLOverWsTrait {
    fn on_connect(&self) -> bool;
}

pub struct GraphQLOverWs {
    phase: GraphQLOverWsPhase,
    handler: Box<dyn GraphQLOverWsTrait>,
}

impl GraphQLOverWs{
    pub fn new(handler: Box<dyn GraphQLOverWsTrait>) -> Self {
        Self {
            phase: GraphQLOverWsPhase::SessionInit,
            handler
        }
    }

    pub fn handle_request<S>(&mut self, request: ClientPayload<S>) {
        match request.type_name {
            ClientConnectionType::ConnectionInit => {
                self.handler.on_connect();
                // todo: return GQL_CONNECTION_ACK + GQL_CONNECTION_KEEP_ALIVE (if used)
                //       or GQL_CONNECTION_ERROR in case of false or thrown exception
                self.phase = GraphQLOverWsPhase::Connected;
            },

            ClientConnectionType::Start => {
                // subscription created
                // Server calls onOperation callback,
                //      and responds with GQL_DATA in case of zero errors,
                //      or GQL_ERROR if there is a problem with the operation
                // (it might also return GQL_ERROR with errors array,
                //  in case of resolvers errors).

                // Server calls onOperationDone if the operation is
                //      a query or mutation (for subscriptions, this called when unsubscribing)
                // Server sends GQL_COMPLETE if the operation is a query or mutation
                //      (for subscriptions, this sent when unsubscribing)
            },
            ClientConnectionType::Stop => {},
            ClientConnectionType::ConnectionTerminate => {},
        }
    }
}

enum GraphQLOverWsPhase {
    SessionInit,
    Connected,
}

#[derive(Deserialize)]
#[serde(bound = "GraphQLPayload<S>: Deserialize<'de>")]
pub struct ClientPayload<S>
    where
        S: ScalarValue + Send + Sync + 'static,


        for<'b> &'b S: ScalarRefValue<'b>,
{
    pub id: Option<String>,
    #[serde(rename(deserialize = "type"))]
    pub type_name: ClientConnectionType,
    pub payload: Option<GraphQLPayload<S>>,
}

#[derive(Debug, Deserialize)]
#[serde(bound = "InputValue<S>: Deserialize<'de>")]
pub struct GraphQLPayload<S>
    where
        S: ScalarValue + Send + Sync + 'static,
        for<'b> &'b S: ScalarRefValue<'b>,
{
    pub variables: Option<InputValue<S>>,
    pub extensions: Option<HashMap<String, String>>,
    #[serde(rename(deserialize = "operationName"))]
    pub operaton_name: Option<String>,
    pub query: Option<String>,
}


// TODO: support everything in caps and with `GQL_` prefix
#[derive(Serialize, Deserialize)]
pub enum ClientConnectionType {
    #[serde(rename = "connection_init")]
    ConnectionInit,
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "connection_terminate")]
    ConnectionTerminate,

}

impl fmt::Display for ClientConnectionType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ClientConnectionType::ConnectionInit => write!(f, "connection_init"),
            ClientConnectionType::Start => write!(f, "start"),
            ClientConnectionType::Stop => write!(f, "stop"),
            ClientConnectionType::ConnectionTerminate => write!(f, "connection_terminate"),
        }
    }
}

// TODO: support everything in caps and with `GQL_` prefix
#[derive(Serialize, Deserialize)]
pub enum ServerConnectionType {
    #[serde(rename = "connection_error")]
    ConnectionError,
    #[serde(rename = "connection_ack")]
    ConnectionAck,
    #[serde(rename = "data")]
    Data,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "connection_keep_alive")]
    ConnectionKeepAlive,
}

impl fmt::Display for ServerConnectionType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ServerConnectionType::ConnectionError => write!(f, "connection_error"),
            ServerConnectionType::ConnectionAck => write!(f, "connection_ack"),
            ServerConnectionType::Data => write!(f, "data"),
            ServerConnectionType::Error => write!(f, "error"),
            ServerConnectionType::Complete => write!(f, "complete"),
            ServerConnectionType::ConnectionKeepAlive => write!(f, "connection_keep_alive"),
        }
    }
}
