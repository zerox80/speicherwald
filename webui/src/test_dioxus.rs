use dioxus::prelude::*;

fn _app() -> Element {
    let selected_items = use_signal(|| std::collections::HashSet::<String>::new());
    let mut last_selected_idx = use_signal(|| None as Option<usize>);
    
    rsx! {
        input {
            r#type: "checkbox",
            checked: selected_items.read().contains("foo"),
            onclick: move |e| {
                let has_shift = e.modifiers().contains(Modifiers::SHIFT);
                println!("Shift: {}", has_shift);
            }
        }
    }
}

fn main() {}
