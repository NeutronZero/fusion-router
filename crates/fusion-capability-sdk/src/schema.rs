use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct SchemaBuilder {
    schema: Option<Value>,
}

impl SchemaBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn schema(mut self, schema: Value) -> Self {
        self.schema = Some(schema);
        self
    }

    #[cfg(feature = "schemars")]
    pub fn derive<T: schemars::JsonSchema>() -> Self {
        let schema = schemars::schema_for!(T);
        Self {
            schema: Some(serde_json::to_value(&schema).unwrap_or_default()),
        }
    }

    pub fn finish(self) -> Value {
        self.schema.unwrap_or(Value::Object(Default::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_schema() {
        let schema = SchemaBuilder::new().finish();
        assert_eq!(schema, json!({}));
    }

    #[test]
    fn explicit_schema() {
        let schema = SchemaBuilder::new()
            .schema(json!({"type": "string"}))
            .finish();
        assert_eq!(schema, json!({"type": "string"}));
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn derive_schema() {
        #[derive(schemars::JsonSchema)]
        struct Input {
            value: String,
        }
        let schema = SchemaBuilder::derive::<Input>().finish();
        assert!(schema.is_object());
    }
}
