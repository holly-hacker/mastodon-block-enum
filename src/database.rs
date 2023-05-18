#![allow(unused)]

use std::{borrow::Cow, collections::BTreeMap};

use color_eyre::eyre::Context;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Default, Clone)]
pub struct DatabaseInstance {
    content: DatabaseContent,
}

impl DatabaseInstance {
    pub fn load(path: &str) -> color_eyre::Result<Self> {
        let content = std::fs::read(path).context("read database file")?;
        let content = serde_json::from_slice(&content).context("deserialize database file")?;
        Ok(Self { content })
    }

    pub fn save(&self, path: &str) -> color_eyre::Result<()> {
        let serialized = serde_json::to_vec_pretty(&self.content).context("serialize database")?;
        std::fs::write(path, serialized).context("write database file")?;
        Ok(())
    }

    pub fn use_namespace(mut self, namespace: &'static str) -> DatabaseAccess {
        if !self.content.0.contains_key(namespace) {
            self.content
                .0
                .insert(namespace.to_string(), Default::default());
        }

        DatabaseAccess {
            namespace,
            db: self,
        }
    }
}

#[derive(Clone)]
pub struct DatabaseAccess {
    namespace: &'static str,
    db: DatabaseInstance,
}

impl DatabaseAccess {
    pub fn get<T: DatabaseObject + DeserializeOwned>(
        &self,
        object_id: &str,
    ) -> color_eyre::Result<Option<T>> {
        self.db.content.get(self.namespace, object_id)
    }

    pub fn iter_keys<T: DatabaseObject>(&mut self) -> impl Iterator<Item = String> + '_ {
        self.db.content.get_keys::<T>(self.namespace)
    }

    pub fn set<T: DatabaseObject + Serialize>(&mut self, value: T) -> bool {
        self.db.content.set(self.namespace, value)
    }

    pub fn pop_namespace(self) -> DatabaseInstance {
        self.db
    }
}

pub trait DatabaseObject {
    const KEY_NAME: &'static str;

    fn get_id(&self) -> Cow<str>;
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct DatabaseContent(BTreeMap<String, BTreeMap<String, serde_json::Value>>);

impl DatabaseContent {
    fn get<T: DatabaseObject + DeserializeOwned>(
        &self,
        namespace: &'static str,
        id: &str,
    ) -> color_eyre::Result<Option<T>> {
        self.0[namespace]
            .get(&get_object_id::<T>(id))
            .cloned()
            .map(|value| {
                serde_json::from_value::<T>(value).context("deserialize object from db on get")
            })
            .transpose()
    }

    fn get_keys<'s, T: DatabaseObject>(
        &'s self,
        namespace: &'static str,
    ) -> impl Iterator<Item = String> + 's {
        let map = &self.0[namespace];
        map.keys().filter_map(|k| {
            k.split_once(':')
                .filter(|(left, _)| *left == T::KEY_NAME)
                .map(|(_, right)| right.to_string())
        })
    }

    fn set<T: DatabaseObject + Serialize>(&mut self, namespace: &str, value: T) -> bool {
        let object_id = get_object_id::<T>(&value.get_id());
        let json_value = serde_json::to_value(value).expect("serialize object for insert in db");

        let namespace = self
            .0
            .get_mut(namespace)
            .expect("get namespace after check");

        namespace.insert(object_id, json_value).is_some()
    }
}

fn get_object_id<T: DatabaseObject>(id: &str) -> String {
    format!("{}:{id}", T::KEY_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct MyDbItem1 {
        pub id: String,
        pub name: String,
    }

    impl DatabaseObject for MyDbItem1 {
        const KEY_NAME: &'static str = "my_db_item";

        fn get_id(&self) -> Cow<str> {
            (&self.id).into()
        }
    }

    #[derive(Serialize, Deserialize)]
    struct MyDbItem2 {
        pub id: String,
    }

    impl DatabaseObject for MyDbItem2 {
        const KEY_NAME: &'static str = "my_db_item_2";

        fn get_id(&self) -> Cow<str> {
            (&self.id).into()
        }
    }

    #[test]
    fn insert_and_read() {
        let db = DatabaseInstance::default();
        let mut dba = db.use_namespace("test_db");
        dba.set(MyDbItem1 {
            id: "123".to_string(),
            name: "Jeffrey".into(),
        });

        assert!(dba.get::<MyDbItem1>("123").unwrap().is_some());
    }

    #[test]
    fn read_no_object() {
        let db = DatabaseInstance::default();
        let dba = db.use_namespace("test_db");

        assert!(dba.get::<MyDbItem1>("123").unwrap().is_none());
    }

    #[test]
    fn get_keys() {
        let db = DatabaseInstance::default();
        let mut dba = db.use_namespace("test_db");

        dba.set(MyDbItem1 {
            id: "123".to_string(),
            name: "Jeffrey".into(),
        });
        dba.set(MyDbItem1 {
            id: "456".to_string(),
            name: "Jimmy".into(),
        });
        dba.set(MyDbItem2 {
            id: "789".to_string(),
        });

        let items = dba.iter_keys::<MyDbItem1>().collect::<Vec<_>>();
        assert_eq!(items, vec![123.to_string(), 456.to_string()]);
    }
}
