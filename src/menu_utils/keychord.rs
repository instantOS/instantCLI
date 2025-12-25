use crossterm::event::KeyCode;

#[derive(Clone, Debug)]
pub struct KeyChord {
    pub description: String,
    pub key: KeyCode,
    pub child: KeyChordChild,
}

impl KeyChord {
    pub fn new(description: impl Into<String>, key: KeyCode, child: KeyChordChild) -> Self {
        Self {
            description: description.into(),
            key,
            child,
        }
    }
}

#[derive(Clone, Debug)]
pub enum KeyChordChild {
    Leaf(KeyChordAction),
    Node(KeyChordNode),
}

#[derive(Clone, Debug)]
pub struct KeyChordNode {
    pub description: String,
    pub chords: Vec<KeyChord>,
}

impl KeyChordNode {
    pub fn new(description: impl Into<String>, chords: Vec<KeyChord>) -> Self {
        Self {
            description: description.into(),
            chords,
        }
    }

    pub fn find_chord(&self, key: &KeyCode) -> Option<&KeyChord> {
        self.chords.iter().find(|chord| chord.key == *key)
    }

    pub fn is_empty(&self) -> bool {
        self.chords.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct KeyChordAction {
    pub id: String,
}

impl KeyChordAction {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}
