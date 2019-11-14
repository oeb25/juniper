//! Utilities for building HTTP endpoints in a library-agnostic manner

pub mod graphiql;
pub mod playground;

#[cfg(feature = "async")]
use std::pin::Pin;

#[cfg(feature = "async")]
use futures::{stream::Stream, stream::StreamExt as _, Poll};
use serde::{
    de::Deserialize,
    ser::{self, Serialize, SerializeMap},
};
use serde_derive::{Deserialize, Serialize};

#[cfg(feature = "async")]
use crate::executor::ValuesStream;
use crate::{
    ast::InputValue,
    executor::{ExecutionError, ValuesIterator},
    value,
    value::{DefaultScalarValue, ScalarRefValue, ScalarValue},
    FieldError, GraphQLError, GraphQLType, Object, RootNode, Value, Variables,
};

/// The expected structure of the decoded JSON document for either POST or GET requests.
///
/// For POST, you can use Serde to deserialize the incoming JSON data directly
/// into this struct - it derives Deserialize for exactly this reason.
///
/// For GET, you will need to parse the query string and extract "query",
/// "operationName", and "variables" manually.
#[derive(Deserialize, Clone, Serialize, PartialEq, Debug)]
pub struct GraphQLRequest<S = DefaultScalarValue>
where
    S: ScalarValue,
{
    query: String,
    #[serde(rename = "operationName")]
    operation_name: Option<String>,
    #[serde(bound(deserialize = "InputValue<S>: Deserialize<'de> + Serialize"))]
    variables: Option<InputValue<S>>,
}

impl<S> GraphQLRequest<S>
where
    S: ScalarValue,
{
    /// Returns the `operation_name` associated with this request.
    pub fn operation_name(&self) -> Option<&str> {
        self.operation_name.as_ref().map(|oper_name| &**oper_name)
    }

    fn variables(&self) -> Variables<S> {
        self.variables
            .as_ref()
            .and_then(|iv| {
                iv.to_object_value().map(|o| {
                    o.into_iter()
                        .map(|(k, v)| (k.to_owned(), v.clone()))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    /// Construct a new GraphQL request from parts
    pub fn new(
        query: String,
        operation_name: Option<String>,
        variables: Option<InputValue<S>>,
    ) -> Self {
        GraphQLRequest {
            query,
            operation_name,
            variables,
        }
    }

    // todo: rename to subscribe
    /// Execute a GraphQL subscription using the specified schema and context
    ///
    /// This is a wrapper around the `subscribe_async` function exposed
    /// at the top level of this crate.
    #[cfg(feature = "async")]
    pub async fn subscribe_async<'a, CtxT, QueryT, MutationT, SubscriptionT>(
        &'a self,
        root_node: &'a RootNode<'a, QueryT, MutationT, SubscriptionT, S>,
        context: &'a CtxT,
        executor: &'a mut crate::executor::SubscriptionsExecutor<'a, CtxT, S>,
    ) -> StreamGraphQLResponse<'a, S>
    where
        S: ScalarValue + Send + Sync + 'static,
        QueryT: crate::GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        QueryT::TypeInfo: Send + Sync,
        MutationT: crate::GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        MutationT::TypeInfo: Send + Sync,
        SubscriptionT: crate::GraphQLSubscriptionTypeAsync<S, Context = CtxT> + Send + Sync,
        SubscriptionT::TypeInfo: Send + Sync,
        CtxT: Send + Sync,
        for<'b> &'b S: ScalarRefValue<'b>,
    {
        let op = self.operation_name();
        let vars = self.variables();
        let res = crate::subscribe_async(&self.query, op, root_node, vars, context, executor).await;

        StreamGraphQLResponse(res)
    }

    /// Execute a GraphQL request using the specified schema and context
    ///
    /// This is a simple wrapper around the `execute` function exposed at the
    /// top level of this crate.
    pub fn execute<'a, CtxT, QueryT, MutationT, SubscriptionT>(
        &'a self,
        root_node: &'a RootNode<QueryT, MutationT, SubscriptionT, S>,
        context: &CtxT,
    ) -> GraphQLResponse<'a, S>
    where
        S: ScalarValue + Send + Sync + 'static,
        QueryT: GraphQLType<S, Context = CtxT>,
        MutationT: GraphQLType<S, Context = CtxT>,
        SubscriptionT: GraphQLType<S, Context = CtxT>,
        for<'b> &'b S: ScalarRefValue<'b>,
    {
        GraphQLResponse(crate::execute(
            &self.query,
            self.operation_name(),
            root_node,
            &self.variables(),
            context,
        ))
    }

    /// Execute a GraphQL request asynchronously using the specified schema and context
    ///
    /// This is a simple wrapper around the `execute_async` function exposed at the
    /// top level of this crate.
    #[cfg(feature = "async")]
    pub async fn execute_async<'a, CtxT, QueryT, MutationT, SubscriptionT>(
        &'a self,
        root_node: &'a RootNode<'a, QueryT, MutationT, SubscriptionT, S>,
        context: &'a CtxT,
    ) -> GraphQLResponse<'a, S>
    where
        S: ScalarValue + Send + Sync + 'static,
        QueryT: crate::GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        QueryT::TypeInfo: Send + Sync,
        MutationT: crate::GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        MutationT::TypeInfo: Send + Sync,
        SubscriptionT: crate::GraphQLSubscriptionTypeAsync<S, Context = CtxT> + Send + Sync,
        SubscriptionT::TypeInfo: Send + Sync,
        CtxT: Send + Sync,
        for<'b> &'b S: ScalarRefValue<'b>,
    {
        let op = self.operation_name();
        let vars = &self.variables();
        let res = crate::execute_async(&self.query, op, root_node, vars, context).await;
        GraphQLResponse(res)
    }
}

/// Simple wrapper around the result from executing a GraphQL query
///
/// This struct implements Serialize, so you can simply serialize this
/// to JSON and send it over the wire. Use the `is_ok` method to determine
/// whether to send a 200 or 400 HTTP status code.
pub struct GraphQLResponse<'a, S = DefaultScalarValue>(
    Result<(Value<S>, Vec<ExecutionError<S>>), GraphQLError<'a>>,
);

/// Wrapper around the result from executing a GraphQL subscription
pub struct IteratorGraphQLResponse<'a, S = DefaultScalarValue>(
    Result<Value<ValuesIterator<'a, S>>, GraphQLError<'a>>,
)
where
    S: 'static;

#[cfg(feature = "async")]
/// Wrapper around the asynchronous result from executing a GraphQL subscription
pub struct StreamGraphQLResponse<'a, S = DefaultScalarValue>(
    Result<Value<ValuesStream<'a, S>>, GraphQLError<'a>>,
)
where
    S: 'static;

impl<'a, S> GraphQLResponse<'a, S>
where
    S: ScalarValue,
{
    /// Constructs new `GraphQLResponse` using the given result
    pub fn from_result(r: Result<(Value<S>, Vec<ExecutionError<S>>), GraphQLError<'a>>) -> Self {
        Self(r)
    }

    /// Constructs an error response outside of the normal execution flow
    pub fn error(error: FieldError<S>) -> Self {
        GraphQLResponse(Ok((Value::null(), vec![ExecutionError::at_origin(error)])))
    }

    /// Was the request successful or not?
    ///
    /// Note that there still might be errors in the response even though it's
    /// considered OK. This is by design in GraphQL.
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }
}

impl<'a, T> Serialize for GraphQLResponse<'a, T>
where
    T: Serialize + ScalarValue,
    Value<T>: Serialize,
    ExecutionError<T>: Serialize,
    GraphQLError<'a>: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        match self.0 {
            Ok((ref res, ref err)) => {
                let mut map = serializer.serialize_map(None)?;

                map.serialize_key("data")?;
                map.serialize_value(res)?;

                if !err.is_empty() {
                    map.serialize_key("errors")?;
                    map.serialize_value(err)?;
                }

                map.end()
            }
            Err(ref err) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_key("errors")?;
                map.serialize_value(err)?;
                map.end()
            }
        }
    }
}

impl<'a, S> IteratorGraphQLResponse<'a, S> {
    /// Convert `IteratorGraphQLResponse` to `Value<ValuesIterator>`
    pub fn into_inner(self) -> Result<Value<ValuesIterator<'a, S>>, GraphQLError<'a>> {
        self.0
    }

    /// Return reference to self's errors (if any)
    pub fn errors<'err>(&'err self) -> Option<&'err GraphQLError<'a>> {
        self.0.as_ref().err()
    }
}

impl<'a, S> IteratorGraphQLResponse<'a, S>
where
    S: value::ScalarValue,
{
    /// Converts `self` into default `Iterator` implementantion.
    /// Is not implemented as `std::iter::IntoIterator` because in some cases
    /// iterator cannot be generated.
    ///
    /// Default `Iterator` implementation provides iterator
    /// based on `Self`'s internal value:
    ///     `Value::Null` - iterator over one wrapped `Value::Null`
    ///     `Value::List` - default implementation is not provided
    ///     `Value::Scalar` - wrapped underlying iterator
    ///     `Value::Object(Value::Scalar(iterator))` - iterator over objects with each field collected.
    ///                                                Stops when at least one field's iterator is finished
    ///     other `Value::Object` - __panics__
    /// Returns None is `Self`'s internal result is error or `Value::List`
    #[allow(clippy::should_implement_trait)]
    pub fn into_iter(self) -> Option<Box<dyn Iterator<Item = GraphQLResponse<'static, S>> + 'a>> {
        let val = match self.0 {
            Ok(val) => val,
            Err(_) => return None,
        };

        match val {
            Value::Null => None,
            Value::Scalar(iter) => {
                Some(Box::new(iter.map(|value| {
                    GraphQLResponse::from_result(Ok((value, vec![])))
                })))
            }
            // TODO: implement these
            Value::List(_) => unimplemented!(),
            Value::Object(mut obj) => unimplemented!(),
        }
    }
}

#[cfg(feature = "async")]
impl<'a, S> StreamGraphQLResponse<'a, S> {
    /// Convert `StreamGraphQLResponse` to `Value<ValuesStream>`
    pub fn into_inner(self) -> Result<Value<ValuesStream<'a, S>>, GraphQLError<'a>> {
        self.0
    }

    /// Return reference to self's errors (if any)
    pub fn errors<'err>(&'err self) -> Option<&'err GraphQLError<'a>> {
        self.0.as_ref().err()
    }
}

#[cfg(feature = "async")]
impl<'a, S> StreamGraphQLResponse<'a, S>
where
    S: value::ScalarValue + Send,
{
    /// Converts `Self` into default `Stream` for implementantion
    ///
    /// Default `Stream` implementantion based on value's type:
    ///     `Value::Null` - stream with a single wrapped `Value::Null`
    ///     `Value::Scalar` - wrapped underlying stream
    ///     `Value::List` - default implementantion is not provided
    ///     `Value::Object(Value::Scalar(stream))` - creates new object out of each returned values.
    ///                                              Stops when at least one stream stops
    ///     other `Value::Object` - default implementation __panics__
    pub fn into_stream(
        self,
    ) -> Option<Pin<Box<dyn futures::Stream<Item = GraphQLResponse<'static, S>> + Send + 'a>>> {
        use std::iter::FromIterator as _;

        let val = match self.0 {
            Ok(val) => val,
            Err(_) => return None,
        };

        match val {
            Value::Null => None,
            Value::Scalar(stream) => {
                Some(Box::pin(stream.map(|value| {
                    GraphQLResponse::from_result(Ok((value, vec![])))
                })))
            }
            // TODO: implement these
            Value::List(_) => unimplemented!(),
            Value::Object(_) => unimplemented!(),
        }
    }
}

#[cfg(any(test, feature = "expose-test-schema"))]
#[allow(missing_docs)]
pub mod tests {
    use serde_json::{self, Value as Json};

    /// Normalized response content we expect to get back from
    /// the http framework integration we are testing.
    #[derive(Debug)]
    pub struct TestResponse {
        pub status_code: i32,
        pub body: Option<String>,
        pub content_type: String,
    }

    /// Normalized way to make requests to the http framework
    /// integration we are testing.
    pub trait HTTPIntegration {
        fn get(&self, url: &str) -> TestResponse;
        fn post(&self, url: &str, body: &str) -> TestResponse;
    }

    #[allow(missing_docs)]
    pub fn run_http_test_suite<T: HTTPIntegration>(integration: &T) {
        println!("Running HTTP Test suite for integration");

        println!("  - test_simple_get");
        test_simple_get(integration);

        println!("  - test_encoded_get");
        test_encoded_get(integration);

        println!("  - test_get_with_variables");
        test_get_with_variables(integration);

        println!("  - test_simple_post");
        test_simple_post(integration);

        println!("  - test_batched_post");
        test_batched_post(integration);

        println!("  - test_invalid_json");
        test_invalid_json(integration);

        println!("  - test_invalid_field");
        test_invalid_field(integration);

        println!("  - test_duplicate_keys");
        test_duplicate_keys(integration);
    }

    fn unwrap_json_response(response: &TestResponse) -> Json {
        serde_json::from_str::<Json>(
            response
                .body
                .as_ref()
                .expect("No data returned from request"),
        )
        .expect("Could not parse JSON object")
    }

    fn test_simple_get<T: HTTPIntegration>(integration: &T) {
        // {hero{name}}
        let response = integration.get("/?query=%7Bhero%7Bname%7D%7D");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type.as_str(), "application/json");

        assert_eq!(
            unwrap_json_response(&response),
            serde_json::from_str::<Json>(r#"{"data": {"hero": {"name": "R2-D2"}}}"#)
                .expect("Invalid JSON constant in test")
        );
    }

    fn test_encoded_get<T: HTTPIntegration>(integration: &T) {
        // query { human(id: "1000") { id, name, appearsIn, homePlanet } }
        let response = integration.get(
            "/?query=query%20%7B%20human(id%3A%20%221000%22)%20%7B%20id%2C%20name%2C%20appearsIn%2C%20homePlanet%20%7D%20%7D");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type.as_str(), "application/json");

        assert_eq!(
            unwrap_json_response(&response),
            serde_json::from_str::<Json>(
                r#"{
                    "data": {
                        "human": {
                            "appearsIn": [
                                "NEW_HOPE",
                                "EMPIRE",
                                "JEDI"
                                ],
                                "homePlanet": "Tatooine",
                                "name": "Luke Skywalker",
                                "id": "1000"
                            }
                        }
                    }"#
            )
            .expect("Invalid JSON constant in test")
        );
    }

    fn test_get_with_variables<T: HTTPIntegration>(integration: &T) {
        // query($id: String!) { human(id: $id) { id, name, appearsIn, homePlanet } }
        // with variables = { "id": "1000" }
        let response = integration.get(
            "/?query=query(%24id%3A%20String!)%20%7B%20human(id%3A%20%24id)%20%7B%20id%2C%20name%2C%20appearsIn%2C%20homePlanet%20%7D%20%7D&variables=%7B%20%22id%22%3A%20%221000%22%20%7D");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, "application/json");

        assert_eq!(
            unwrap_json_response(&response),
            serde_json::from_str::<Json>(
                r#"{
                    "data": {
                        "human": {
                            "appearsIn": [
                                "NEW_HOPE",
                                "EMPIRE",
                                "JEDI"
                                ],
                                "homePlanet": "Tatooine",
                                "name": "Luke Skywalker",
                                "id": "1000"
                            }
                        }
                    }"#
            )
            .expect("Invalid JSON constant in test")
        );
    }

    fn test_simple_post<T: HTTPIntegration>(integration: &T) {
        let response = integration.post("/", r#"{"query": "{hero{name}}"}"#);

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, "application/json");

        assert_eq!(
            unwrap_json_response(&response),
            serde_json::from_str::<Json>(r#"{"data": {"hero": {"name": "R2-D2"}}}"#)
                .expect("Invalid JSON constant in test")
        );
    }

    fn test_batched_post<T: HTTPIntegration>(integration: &T) {
        let response = integration.post(
            "/",
            r#"[{"query": "{hero{name}}"}, {"query": "{hero{name}}"}]"#,
        );

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, "application/json");

        assert_eq!(
            unwrap_json_response(&response),
            serde_json::from_str::<Json>(
                r#"[{"data": {"hero": {"name": "R2-D2"}}}, {"data": {"hero": {"name": "R2-D2"}}}]"#
            )
            .expect("Invalid JSON constant in test")
        );
    }

    fn test_invalid_json<T: HTTPIntegration>(integration: &T) {
        let response = integration.get("/?query=blah");
        assert_eq!(response.status_code, 400);
        let response = integration.post("/", r#"blah"#);
        assert_eq!(response.status_code, 400);
    }

    fn test_invalid_field<T: HTTPIntegration>(integration: &T) {
        // {hero{blah}}
        let response = integration.get("/?query=%7Bhero%7Bblah%7D%7D");
        assert_eq!(response.status_code, 400);
        let response = integration.post("/", r#"{"query": "{hero{blah}}"}"#);
        assert_eq!(response.status_code, 400);
    }

    fn test_duplicate_keys<T: HTTPIntegration>(integration: &T) {
        // {hero{name}}
        let response = integration.get("/?query=%7B%22query%22%3A%20%22%7Bhero%7Bname%7D%7D%22%2C%20%22query%22%3A%20%22%7Bhero%7Bname%7D%7D%22%7D");
        assert_eq!(response.status_code, 400);
        let response = integration.post(
            "/",
            r#"
            {"query": "{hero{name}}", "query": "{hero{name}}"}
        "#,
        );
        assert_eq!(response.status_code, 400);
    }
}
