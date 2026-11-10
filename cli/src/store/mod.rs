mod atomic;
mod fs_store;

pub use fs_store::FsStore;

use std::sync::Arc;

use doer_core::store::{Loaded, ProjectFile, Store, StoreError, StoreSnapshot};
use doer_core::{ProjectId, Todo};

#[derive(Clone, Debug)]
pub struct SharedStore(pub Arc<FsStore>);

impl Store for SharedStore {
    fn load(&self) -> Loaded<StoreSnapshot> {
        self.0.load()
    }
    fn save_all_todos(&self, todos: &[Todo]) -> Result<(), StoreError> {
        self.0.save_all_todos(todos)
    }
    fn save_project(&self, file: &ProjectFile) -> Result<(), StoreError> {
        self.0.save_project(file)
    }
    fn delete_project(&self, id: &ProjectId) -> Result<(), StoreError> {
        self.0.delete_project(id)
    }
}
