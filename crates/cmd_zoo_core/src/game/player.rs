use uuid::Uuid;

pub const DEFAULT_PLAYER_NAME: &str = "Zookeeper";
pub const MAX_PLAYER_NAME_LEN: usize = 24;

#[derive(Clone, Debug)]
pub struct Player {
    pub id: Uuid,
    pub name: String,
}

impl Player {
    pub fn new_default() -> Self {
        Self {
            id: crate::game::ids::new_id(),
            name: DEFAULT_PLAYER_NAME.to_string(),
        }
    }

    pub fn rename(&mut self, name: impl Into<String>) {
        let mut n: String = name.into();
        n = n.trim().to_string();
        if n.is_empty() {
            n = DEFAULT_PLAYER_NAME.to_string();
        }
        n.truncate(MAX_PLAYER_NAME_LEN);
        self.name = n;
    }
}
