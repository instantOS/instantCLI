I want the menu utils to support keyhords. These are for now not passed from
outside but just created and used as internal data structures. Keychords are
kind of like a tree, structured by their keys

Pseudocode following

```
struct KeyChord {
    description: String,
    key: Key,
    child: KeyChild,
}

enum KeyChild {
    Leaf(Action),
    Node(Vec<KeyChord>),
}

```

Nodes should also have descriptions


Keep in mind during non-keychord usage, the TUI should NOT listen for input
events, as it would take them away from spawned fzf instances.


The TUI should work the following: 
- Display a list of all keychords at the current level (key and description)
- pressing a key should go to the node for that key. 
- repeat for children of the current node, if the node is a leaf, execute the action

for now, build a demo under `ins menu chorddemo` that shows a hardcoded keychord
tree and allows navigating it, an action should print the action ID. 

