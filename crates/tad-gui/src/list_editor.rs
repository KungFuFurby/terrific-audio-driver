//! List Editors

// SPDX-FileCopyrightText: © 2023 Marcus Rowe <undisbeliever@gmail.com>
//
// SPDX-License-Identifier: MIT

use crate::compiler_thread::{self, ItemChanged, ItemId};
use crate::helpers::{ch_units_to_width, SetActive};
use crate::names::{DeduplicatedNameVec, NameDeduplicator};
use crate::tables;
use crate::GuiMessage;

use fltk::button::Button;
use fltk::group::{Pack, PackType};
use fltk::prelude::{GroupExt, WidgetExt};

use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Deref;
use std::rc::Rc;

// A ListMessage MUST ONLY be called once per frame
// (to prevent a potential infinite `ListMessage::ItemSelected` loop)
#[derive(Debug)]
pub enum ListMessage<T> {
    ClearSelection,
    ItemSelected(usize),

    // The `SelectedItemEdited` message has been removed because clicking on a TrTable list item
    // will cause the `ItemSelected` message to be sent before the `SelectedItemEdited` event.
    ItemEdited(usize, T),

    Add(T),
    AddMultiple(Vec<T>),
    Clone(usize),
    Remove(usize),
    MoveToTop(usize),
    MoveUp(usize),
    MoveDown(usize),
    MoveToBottom(usize),

    // Only adds the item if the list does not contain ItemId.
    AddWithItemId(ItemId, T),
}

pub trait ListEditor<T> {
    fn list_edited(&mut self, action: &ListAction<T>);

    fn clear_selected(&mut self);
    fn set_selected(&mut self, index: usize, id: ItemId, value: &T);
}

pub trait CompilerOutputGui<T> {
    fn set_compiler_output(&mut self, index: usize, compiler_output: &Option<T>);
    fn set_selected_compiler_output(&mut self, compiler_output: &Option<T>);
}

#[derive(Debug)]
pub enum ListAction<T> {
    None,
    Add(usize, T),
    AddMultiple(usize, Vec<T>),
    Remove(usize),
    Edit(usize, T),
    Move(usize, usize),
}

impl<T> ListAction<T> {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Preforms the ListAction on a list
///
/// #Panics
/// Panics if an index is out-of-bounds.  For safety, ListAction should be the only thing that modifies `list`.
pub fn process_list_action<T>(list: &mut Vec<T>, action: &ListAction<T>)
where
    T: Clone,
{
    process_list_action_map(
        list,
        action,
        // add
        |item| item.clone(),
        // edit
        |item, new_value| *item = new_value.clone(),
    );
}

/// Preforms the ListAction on a list not of type T.
///
/// #Panics
/// Panics if an index is out-of-bounds.  For safety, ListAction should be the only thing that modifies `list`.
pub fn process_list_action_map<T, U>(
    list: &mut Vec<U>,
    action: &ListAction<T>,
    add: impl Fn(&T) -> U,
    edit: impl FnOnce(&mut U, &T),
) {
    match action {
        ListAction::None => (),
        ListAction::Add(i, item) => list.insert(*i, add(item)),
        ListAction::AddMultiple(i, items) => {
            list.splice(i..i, items.iter().map(add));
        }
        ListAction::Remove(i) => {
            list.remove(*i);
        }
        ListAction::Edit(i, item) => edit(&mut list[*i], item),
        &ListAction::Move(from, to) => {
            if from < to {
                // Move down
                for i in from..to {
                    list.swap(i, i + 1);
                }
            } else {
                // Move up
                for i in (to + 1..=from).rev() {
                    list.swap(i, i - 1);
                }
            }
        }
    }
}

/// A `Vec` that can only be resized or reordered by a `ListAction<T>`
#[derive(Debug, Default, Clone)]
pub struct LaVec<T>(Vec<T>);

impl<T> LaVec<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn from_vec(v: Vec<T>) -> Self {
        Self(v)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.0.get_mut(index)
    }

    #[allow(dead_code)]
    pub fn process(&mut self, action: &ListAction<T>)
    where
        T: Clone,
    {
        process_list_action(&mut self.0, action);
    }

    pub fn process_map<U>(
        &mut self,
        action: &ListAction<U>,
        add: impl Fn(&U) -> T,
        edit: impl FnOnce(&mut T, &U),
    ) {
        process_list_action_map(&mut self.0, action, add, edit);
    }
}

impl<T> Deref for LaVec<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

pub trait ListState
where
    Self::Item: Clone + PartialEq<Self::Item> + NameDeduplicator,
{
    type Item;

    fn selected(&self) -> Option<usize>;
    fn len(&self) -> usize;
    fn can_add(&self) -> bool;
    fn can_add_multiple(&self, count: usize) -> bool;
    fn item_iter(&self) -> impl Iterator<Item = &Self::Item>;
}

pub trait CompilerOutput {
    fn is_valid(&self) -> bool;
}

impl<T, E> CompilerOutput for Result<T, E> {
    fn is_valid(&self) -> bool {
        self.is_ok()
    }
}

pub struct ListWithCompilerOutput<T, O>
where
    T: Clone + PartialEq<T> + NameDeduplicator,
    O: CompilerOutput,
{
    max_size: usize,
    list: LaVec<(ItemId, T)>,
    compiler_output: Vec<Option<O>>,
    error_set: HashSet<ItemId>,
    selected: Option<usize>,
}

impl<T, O> ListState for ListWithCompilerOutput<T, O>
where
    T: Clone + PartialEq<T> + NameDeduplicator,
    O: CompilerOutput,
{
    type Item = T;

    fn len(&self) -> usize {
        self.list.len()
    }

    fn selected(&self) -> Option<usize> {
        self.selected
    }

    fn can_add(&self) -> bool {
        self.list.len() < self.max_size
    }

    fn can_add_multiple(&self, count: usize) -> bool {
        self.list.len() + count <= self.max_size
    }

    fn item_iter(&self) -> impl Iterator<Item = &T> {
        self.list.iter().map(|(_id, item)| item)
    }
}

impl<T, O> ListWithCompilerOutput<T, O>
where
    T: Clone + PartialEq<T> + NameDeduplicator,
    O: CompilerOutput,
{
    pub fn new(list: DeduplicatedNameVec<T>, max_size: usize) -> Self {
        let list = LaVec::from_vec(
            list.into_vec()
                .into_iter()
                .map(|i| (ItemId::new(), i))
                .collect(),
        );

        // Compiler output is not cloneable
        let compiler_output = std::iter::repeat_with(|| None).take(list.len()).collect();

        Self {
            max_size,
            list,
            compiler_output,
            error_set: HashSet::new(),
            selected: None,
        }
    }

    pub fn all_valid(&self) -> bool {
        self.error_set.is_empty()
    }

    pub fn set_compiler_output(
        &mut self,
        id: ItemId,
        co: O,
        editor: &mut impl CompilerOutputGui<O>,
    ) {
        match co.is_valid() {
            true => self.error_set.remove(&id),
            false => self.error_set.insert(id),
        };

        let co = Some(co);

        if let Some(index) = self.id_to_index(id) {
            editor.set_compiler_output(index, &co);

            if self.selected == Some(index) {
                editor.set_selected_compiler_output(&co);
            }
            if let Some(co_item) = self.compiler_output.get_mut(index) {
                *co_item = co;
            }
        }
    }

    #[must_use]
    pub fn process<Editor>(
        &mut self,
        m: ListMessage<T>,
        editor: &mut Editor,
    ) -> (ListAction<T>, Option<ItemChanged<T>>)
    where
        Editor: ListEditor<T> + CompilerOutputGui<O>,
    {
        let update_list = |list: &mut LaVec<(ItemId, T)>, a: &ListAction<T>| {
            list.process_map(
                a,
                // new
                |v: &T| (ItemId::new(), v.clone()),
                // edit
                |e, v: &T| e.1 = v.clone(),
            );
        };

        let (action, c) = match m {
            ListMessage::ClearSelection => {
                self.clear_selection(editor);
                (ListAction::None, None)
            }
            ListMessage::ItemSelected(index) => {
                self.set_selected(index, editor);
                (ListAction::None, None)
            }

            ListMessage::ItemEdited(index, mut new_value) => {
                if let Some((id, item)) = self.list.get(index) {
                    if *item != new_value {
                        if NameDeduplicator::test_name_changed(item, &new_value) {
                            NameDeduplicator::dedupe_name(&mut new_value, &self.list, Some(index));
                        }

                        let c_message = ItemChanged::AddedOrEdited(*id, new_value.clone());

                        let action = ListAction::Edit(index, new_value);
                        update_list(&mut self.list, &action);
                        (action, Some(c_message))
                    } else {
                        (ListAction::None, None)
                    }
                } else {
                    (ListAction::None, None)
                }
            }

            ListMessage::Add(mut item) => {
                if self.can_add() {
                    NameDeduplicator::dedupe_name(&mut item, &self.list, None);

                    let i = self.list.len();
                    let action = ListAction::Add(i, item);
                    update_list(&mut self.list, &action);
                    let c_message = self
                        .list
                        .get(i)
                        .map(|(id, item)| ItemChanged::AddedOrEdited(*id, item.clone()));

                    (action, c_message)
                } else {
                    (ListAction::None, None)
                }
            }

            ListMessage::AddWithItemId(id, mut item) => {
                if self.can_add() && !self.contains_id(id) {
                    NameDeduplicator::dedupe_name(&mut item, &self.list, None);

                    let i = self.list.len();
                    let action = ListAction::Add(i, item);

                    self.list.process_map(
                        &action,
                        // new
                        |v: &T| (id, v.clone()),
                        // edit
                        |_, _| (),
                    );

                    let c_message = self
                        .list
                        .get(i)
                        .map(|(id, item)| ItemChanged::AddedOrEdited(*id, item.clone()));

                    assert!(self.contains_id(id));

                    (action, c_message)
                } else {
                    (ListAction::None, None)
                }
            }

            ListMessage::AddMultiple(mut items) => {
                if self.can_add_multiple(items.len()) {
                    let old_size = self.list.len();

                    let mut new_items_with_id = Vec::with_capacity(items.len());

                    // Must add and deduplicate items one at a time to ensure names are unique.
                    for item in &mut items {
                        NameDeduplicator::dedupe_name(item, &self.list, None);

                        let i = self.list.len();
                        let action = ListAction::Add(i, item.clone());
                        update_list(&mut self.list, &action);

                        if let Some(c) = self.list.get(i) {
                            new_items_with_id.push(c.clone());
                        }
                    }

                    let action = ListAction::AddMultiple(old_size, items);
                    let c_message = Some(ItemChanged::MultipleAddedOrEdited(new_items_with_id));

                    (action, c_message)
                } else {
                    (ListAction::None, None)
                }
            }

            ListMessage::Clone(index) => {
                if let (Some(item), true) = (self.list.get(index), self.can_add()) {
                    let mut item = item.1.clone();
                    NameDeduplicator::dedupe_name(&mut item, &self.list, None);

                    let i = index + 1;
                    let action = ListAction::Add(i, item);

                    update_list(&mut self.list, &action);
                    let c_message = self
                        .list
                        .get(i)
                        .map(|(id, item)| ItemChanged::AddedOrEdited(*id, item.clone()));

                    (action, c_message)
                } else {
                    (ListAction::None, None)
                }
            }
            ListMessage::Remove(index) => {
                if let Some(item) = self.list.get(index) {
                    let c_message = ItemChanged::Removed(item.0);

                    let action = ListAction::Remove(index);
                    update_list(&mut self.list, &action);

                    (action, Some(c_message))
                } else {
                    (ListAction::None, None)
                }
            }
            ListMessage::MoveToTop(index) => {
                if index > 0 && index < self.list.len() {
                    let action = ListAction::Move(index, 0);
                    update_list(&mut self.list, &action);

                    (action, None)
                } else {
                    (ListAction::None, None)
                }
            }
            ListMessage::MoveUp(index) => {
                if index > 0 && index < self.list.len() {
                    let action = ListAction::Move(index, index - 1);
                    update_list(&mut self.list, &action);

                    (action, None)
                } else {
                    (ListAction::None, None)
                }
            }
            ListMessage::MoveDown(index) => {
                if index + 1 < self.list.len() {
                    let action = ListAction::Move(index, index + 1);
                    update_list(&mut self.list, &action);

                    (action, None)
                } else {
                    (ListAction::None, None)
                }
            }
            ListMessage::MoveToBottom(index) => {
                if index + 1 < self.list.len() {
                    let action = ListAction::Move(index, self.list.len() - 1);
                    update_list(&mut self.list, &action);

                    (action, None)
                } else {
                    (ListAction::None, None)
                }
            }
        };

        process_list_action_map(
            &mut self.compiler_output,
            &action,
            // new
            |_| None,
            // edit
            |e, _| *e = None,
        );

        editor.list_edited(&action);
        match &action {
            ListAction::None => (),

            ListAction::Add(index, _) => {
                self.set_selected(*index, editor);
            }
            ListAction::AddMultiple(index, _) => {
                self.set_selected(*index, editor);
            }
            ListAction::Remove(index) => {
                if self.selected == Some(*index) {
                    self.clear_selection(editor);
                }
            }
            ListAction::Move(from, to) => {
                if self.selected == Some(*from) {
                    self.set_selected(*to, editor);
                }
            }
            ListAction::Edit(index, _) => {
                if let Some(co) = self.compiler_output.get_mut(*index) {
                    // Clear stored compiler output when an item is edited
                    *co = None;
                }
                // `editor.set_compiler_output` and `editor.set_selected_compiler_output`
                // are not called here to prevent an annoying flash in tables and the Sound Effect console.
            }
        }

        (action, c)
    }

    fn set_selected<Editor>(&mut self, index: usize, editor: &mut Editor)
    where
        Editor: ListEditor<T> + CompilerOutputGui<O>,
    {
        match self.get_with_id(index) {
            Some((id, item)) => {
                editor.set_selected(index, id, item);
                self.selected = Some(index);

                let co = self.compiler_output.get(index).unwrap_or(&None);
                editor.set_selected_compiler_output(co);
            }
            None => {
                self.clear_selection(editor);
            }
        };
    }

    fn clear_selection<Editor>(&mut self, editor: &mut Editor)
    where
        Editor: ListEditor<T> + CompilerOutputGui<O>,
    {
        self.selected = None;
        editor.clear_selected();
        editor.set_selected_compiler_output(&None);
    }

    pub fn iter(&self) -> impl Iterator<Item = &(ItemId, T)> {
        self.list.iter()
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.list.get(index).map(|(_id, item)| item)
    }

    pub fn get_with_id(&self, index: usize) -> Option<(ItemId, &T)> {
        self.list.get(index).map(|(id, item)| (*id, item))
    }

    fn id_to_index(&self, id: ItemId) -> Option<usize> {
        self.list.iter().position(|i| i.0 == id)
    }

    fn contains_id(&self, id: ItemId) -> bool {
        self.list.iter().any(|i| i.0 == id)
    }

    pub fn get_id(&self, id: ItemId) -> Option<(usize, &T)> {
        self.list
            .iter()
            .enumerate()
            .find(|(_, i)| i.0 == id)
            .map(|(id, i)| (id, &i.1))
    }

    #[must_use]
    pub fn replace_all_vec(&self) -> compiler_thread::ReplaceAllVec<T> {
        compiler_thread::ReplaceAllVec::new(self.list.0.clone())
    }
}

pub struct ListPairWithCompilerOutputs<T1, O1, T2, O2>
where
    T1: Clone + PartialEq<T1> + NameDeduplicator,
    O1: CompilerOutput,
    T2: Clone + PartialEq<T2> + NameDeduplicator,
    O2: CompilerOutput,
{
    list1: ListWithCompilerOutput<T1, O1>,
    list2: ListWithCompilerOutput<T2, O2>,
}

#[inline]
fn list_pair_process<T1, O1, T2, O2, Editor>(
    m: ListMessage<T1>,
    list1: &mut ListWithCompilerOutput<T1, O1>,
    list2: &mut ListWithCompilerOutput<T2, O2>,
    editor: &mut Editor,
) -> (ListAction<T1>, Option<ItemChanged<T1>>)
where
    T1: Clone + PartialEq<T1> + NameDeduplicator + std::fmt::Debug,
    O1: CompilerOutput,
    T2: Clone + PartialEq<T2> + NameDeduplicator + std::fmt::Debug,
    O2: CompilerOutput,
    Editor: ListEditor<T1> + CompilerOutputGui<O1> + ListEditor<T2> + CompilerOutputGui<O2>,
{
    let can_add = |count: usize| -> bool {
        debug_assert!(list1.max_size == list2.max_size);

        let max_size = list1.max_size;
        let len = list1.len() + list2.len();

        len + count <= max_size
    };
    let can_do_message = match &m {
        ListMessage::Add(..) => can_add(1),
        ListMessage::AddWithItemId(..) => can_add(1),
        ListMessage::AddMultiple(vec) => can_add(vec.len()),
        ListMessage::Clone(..) => can_add(1),

        ListMessage::ClearSelection
        | ListMessage::ItemSelected(..)
        | ListMessage::ItemEdited(..)
        | ListMessage::Remove(..)
        | ListMessage::MoveToTop(..)
        | ListMessage::MoveUp(..)
        | ListMessage::MoveDown(..)
        | ListMessage::MoveToBottom(..) => true,
    };
    if !can_do_message {
        return (ListAction::None, None);
    }

    let clear_list2_selection = match &m {
        // Not changing list2 selection when a list1 item is unselected
        ListMessage::ClearSelection => false,

        // Not changing list2 selection for ItemEdited as it could have been
        // sent because the user selected a list2 item.
        ListMessage::ItemEdited(..) => false,

        ListMessage::Add(..)
        | ListMessage::AddWithItemId(..)
        | ListMessage::AddMultiple(_)
        | ListMessage::Clone(_)
        | ListMessage::ItemSelected(_)
        | ListMessage::Remove(_)
        | ListMessage::MoveToTop(_)
        | ListMessage::MoveUp(_)
        | ListMessage::MoveDown(_)
        | ListMessage::MoveToBottom(_) => true,
    };

    // ::TODO deduplicate name::

    let out = list1.process(m, editor);

    // Must clear list2 **after** list1 is processed to prevent a flash in the Samples Tab.
    // (The flash was caused by the GUI disabling the widgets, then switching editors on the next frame)
    if clear_list2_selection {
        list2.clear_selection(editor);
    }
    out
}

impl<T1, O1, T2, O2> ListPairWithCompilerOutputs<T1, O1, T2, O2>
where
    T1: Clone + PartialEq<T1> + NameDeduplicator + std::fmt::Debug,
    O1: CompilerOutput,
    T2: Clone + PartialEq<T2> + NameDeduplicator + std::fmt::Debug,
    O2: CompilerOutput,
{
    pub fn new(
        list1: DeduplicatedNameVec<T1>,
        list2: DeduplicatedNameVec<T2>,
        max_size: usize,
    ) -> Self {
        let list1 = ListWithCompilerOutput::new(list1, max_size);
        let list2 = ListWithCompilerOutput::new(list2, max_size);

        Self { list1, list2 }
    }

    pub fn list1(&self) -> &ListWithCompilerOutput<T1, O1> {
        &self.list1
    }
    pub fn list2(&self) -> &ListWithCompilerOutput<T2, O2> {
        &self.list2
    }

    pub fn clear_selection<Editor>(&mut self, editor: &mut Editor)
    where
        Editor: ListEditor<T1> + CompilerOutputGui<O1> + ListEditor<T2> + CompilerOutputGui<O2>,
    {
        if self.list1.selected().is_some() {
            self.list1.clear_selection(editor);
        }
        if self.list2.selected().is_some() {
            self.list2.clear_selection(editor);
        }
    }

    #[must_use]
    pub fn process1<Editor>(
        &mut self,
        m: ListMessage<T1>,
        editor: &mut Editor,
    ) -> (ListAction<T1>, Option<ItemChanged<T1>>)
    where
        Editor: ListEditor<T1> + CompilerOutputGui<O1> + ListEditor<T2> + CompilerOutputGui<O2>,
    {
        list_pair_process(m, &mut self.list1, &mut self.list2, editor)
    }

    #[must_use]
    pub fn process2<Editor>(
        &mut self,
        m: ListMessage<T2>,
        editor: &mut Editor,
    ) -> (ListAction<T2>, Option<ItemChanged<T2>>)
    where
        Editor: ListEditor<T1> + CompilerOutputGui<O1> + ListEditor<T2> + CompilerOutputGui<O2>,
    {
        list_pair_process(m, &mut self.list2, &mut self.list1, editor)
    }

    pub fn set_compiler_output1(
        &mut self,
        id: ItemId,
        co: O1,
        editor: &mut impl CompilerOutputGui<O1>,
    ) {
        self.list1.set_compiler_output(id, co, editor)
    }

    pub fn set_compiler_output2(
        &mut self,
        id: ItemId,
        co: O2,
        editor: &mut impl CompilerOutputGui<O2>,
    ) {
        self.list2.set_compiler_output(id, co, editor)
    }
}

pub struct ListButtons {
    pub pack: Pack,

    pub max_size: usize,

    pub add: Button,
    pub clone: Option<Button>,
    pub remove: Button,
    pub move_top: Button,
    pub move_up: Button,
    pub move_down: Button,
    pub move_bottom: Button,
}

impl ListButtons {
    pub fn new(type_name: &str, max_size: usize, show_clone: bool) -> Self {
        let mut pack = Pack::default().with_type(PackType::Horizontal);

        let button_label_size = pack.label_size() * 8 / 10;
        pack.set_label_size(button_label_size);

        let button_size = ch_units_to_width(&pack, 4);

        let button = |label: &str, tooltip: String| {
            let mut b = Button::default()
                .with_size(button_size, button_size)
                .with_label(label);
            b.set_tooltip(&tooltip);
            b.set_label_size(button_label_size);
            b
        };

        let add = button("@add", format!("Add {}", type_name));
        let clone = if show_clone {
            Some(button("@clone", format!("Clone {}", type_name)))
        } else {
            None
        };
        let remove = button("@remove", format!("Remove {}", type_name));
        let move_top = button("@top", format!("Move {} to top", type_name));
        let move_up = button("@up", format!("Move {} up", type_name));
        let move_down = button("@down", format!("Move {} down", type_name));
        let move_bottom = button("@bottom", format!("Move {} to bottom", type_name));

        pack.end();

        let mut out = Self {
            pack,
            max_size,
            add,
            clone,
            remove,
            move_top,
            move_up,
            move_down,
            move_bottom,
        };
        out.deactivate_all();
        out
    }

    fn selected_changed(&mut self, index: usize, list_len: usize) {
        let can_add = list_len < self.max_size;

        self.add.set_active(can_add);

        if let Some(c) = &mut self.clone {
            c.set_active(can_add);
        }
        self.remove.activate();

        if index > 0 {
            self.move_top.activate();
            self.move_up.activate();
        } else {
            self.move_top.deactivate();
            self.move_up.deactivate();
        }

        if index + 1 < list_len {
            self.move_down.activate();
            self.move_bottom.activate();
        } else {
            self.move_down.deactivate();
            self.move_bottom.deactivate();
        }
    }

    fn selected_clear(&mut self, list_len: usize) {
        let can_add = list_len < self.max_size;

        self.add.set_active(can_add);

        if let Some(c) = &mut self.clone {
            c.deactivate();
        }
        self.remove.deactivate();
        self.move_top.deactivate();
        self.move_up.deactivate();
        self.move_down.deactivate();
        self.move_bottom.deactivate();
    }

    pub fn update_buttons(&mut self, selected: Option<usize>, list_len: usize) {
        match selected {
            Some(i) => self.selected_changed(i, list_len),
            None => self.selected_clear(list_len),
        }
    }

    pub fn deactivate_all(&mut self) {
        self.add.deactivate();

        if let Some(c) = &mut self.clone {
            c.deactivate();
        }
        self.remove.deactivate();
        self.move_top.deactivate();
        self.move_up.deactivate();
        self.move_down.deactivate();
        self.move_bottom.deactivate();
    }
}

pub enum TableAction {
    None,
    OpenEditor,
    Send(GuiMessage),
}

pub trait TableMapping
where
    Self::DataType: Sized + Clone + std::cmp::PartialEq<Self::DataType>,
    Self::RowType: tables::TableRow + 'static,
{
    type DataType;
    type RowType;

    const MAX_SIZE: usize;

    const CAN_CLONE: bool;
    const CAN_EDIT: bool;

    fn headers() -> Vec<String>;
    fn type_name() -> &'static str;

    fn add_clicked() -> GuiMessage;
    fn to_message(lm: ListMessage<Self::DataType>) -> GuiMessage;

    fn new_row(d: &Self::DataType) -> Self::RowType;
    fn edit_row(r: &mut Self::RowType, d: &Self::DataType) -> bool;

    fn table_event(event: tables::TableEvent, row: usize, col: i32) -> TableAction {
        let _ = (event, row, col);
        TableAction::None
    }

    fn commit_edited_value(index: usize, col: i32, value: String) -> Option<GuiMessage> {
        let _ = (index, col, value);
        None
    }
}

pub trait TableCompilerOutput
where
    Self: TableMapping,
{
    type CompilerOutputType;

    fn set_row_state(r: &mut Self::RowType, co: &Option<Self::CompilerOutputType>) -> bool;
}

pub struct ListEditorTable<T>
where
    T: TableMapping,
{
    list_buttons_pack: Pack,
    // Must store list_buttons in a separate Rc to prevent a BorrowMutError in set_selection_changed_callback
    list_buttons: Rc<RefCell<ListButtons>>,

    table: Rc<RefCell<tables::TrTable<T::RowType>>>,
}

impl<T> ListEditorTable<T>
where
    T: TableMapping,
    T::DataType: NameDeduplicator,
{
    pub fn new(sender: fltk::app::Sender<GuiMessage>) -> Self {
        let list_buttons = Rc::new(RefCell::new(ListButtons::new(
            T::type_name(),
            T::MAX_SIZE,
            T::CAN_CLONE,
        )));
        let table = Rc::new(RefCell::new(tables::TrTable::new(T::headers())));

        let mut t = table.borrow_mut();
        let mut lb = list_buttons.borrow_mut();

        t.set_selection_changed_callback({
            let sender = sender.clone();
            let list_buttons = list_buttons.clone();
            move |selected, n_rows, user_selection| {
                let mut lb = list_buttons.borrow_mut();

                lb.update_buttons(selected, n_rows);

                if user_selection {
                    match selected {
                        Some(i) => sender.send(T::to_message(ListMessage::ItemSelected(i))),
                        None => sender.send(T::to_message(ListMessage::ClearSelection)),
                    }
                }
            }
        });

        if T::CAN_EDIT {
            t.enable_cell_editing({
                // Commit edited value
                let s = sender.clone();
                move |index, col, value| {
                    if let Some(m) = T::commit_edited_value(index, col, value) {
                        s.send(m);
                    }
                }
            });
        }

        t.set_callback({
            let s = sender.clone();
            move |ev, row, col| match T::table_event(ev, row, col) {
                TableAction::None => false,
                TableAction::OpenEditor => true,
                TableAction::Send(m) => {
                    s.send(m);
                    false
                }
            }
        });

        lb.add.set_callback({
            let s = sender.clone();
            move |_| s.send(T::add_clicked())
        });
        if let Some(b) = &mut lb.clone {
            b.set_callback({
                let s = sender.clone();
                let table = table.clone();
                move |_| {
                    if let Some(i) = table.borrow().selected_row() {
                        s.send(T::to_message(ListMessage::Clone(i)))
                    }
                }
            });
        }
        lb.remove.set_callback({
            let s = sender.clone();
            let table = table.clone();
            move |_| {
                if let Some(i) = table.borrow().selected_row() {
                    s.send(T::to_message(ListMessage::Remove(i)))
                }
            }
        });
        lb.move_top.set_callback({
            let s = sender.clone();
            let table = table.clone();
            move |_| {
                if let Some(i) = table.borrow().selected_row() {
                    s.send(T::to_message(ListMessage::MoveToTop(i)))
                }
            }
        });
        lb.move_up.set_callback({
            let s = sender.clone();
            let table = table.clone();
            move |_| {
                if let Some(i) = table.borrow().selected_row() {
                    s.send(T::to_message(ListMessage::MoveUp(i)))
                }
            }
        });
        lb.move_down.set_callback({
            let s = sender.clone();
            let table = table.clone();
            move |_| {
                if let Some(i) = table.borrow().selected_row() {
                    s.send(T::to_message(ListMessage::MoveDown(i)))
                }
            }
        });
        lb.move_bottom.set_callback({
            let s = sender;
            let table = table.clone();
            move |_| {
                if let Some(i) = table.borrow().selected_row() {
                    s.send(T::to_message(ListMessage::MoveToBottom(i)))
                }
            }
        });
        Self::update_list_buttons(&mut lb, &t);

        let list_buttons_pack = lb.pack.clone();

        drop(lb);
        drop(t);

        Self {
            list_buttons_pack,
            list_buttons,
            table,
        }
    }

    pub fn new_from_slice(data: &[T::DataType], sender: fltk::app::Sender<GuiMessage>) -> Self {
        let out = Self::new(sender);

        {
            let mut t = out.table.borrow_mut();
            let mut lb = out.list_buttons.borrow_mut();

            t.edit_table(|v| v.extend(data.iter().map(T::new_row)));

            Self::update_list_buttons(&mut lb, &t);
        }

        out
    }

    pub fn new_with_data(
        state: &impl ListState<Item = T::DataType>,
        sender: fltk::app::Sender<GuiMessage>,
    ) -> Self {
        let mut out = Self::new(sender);
        out.replace(state);
        out
    }

    pub fn replace(&mut self, state: &impl ListState<Item = T::DataType>) {
        let mut t = self.table.borrow_mut();
        let mut lb = self.list_buttons.borrow_mut();

        t.edit_table(|v| {
            *v = state.item_iter().map(T::new_row).collect();
        });

        match state.selected() {
            Some(i) => t.set_selected(i),
            None => t.clear_selected(),
        }

        Self::update_list_buttons(&mut lb, &t);
    }

    fn update_list_buttons(list_buttons: &mut ListButtons, table: &tables::TrTable<T::RowType>) {
        list_buttons.update_buttons(table.selected_row(), table.n_rows());
    }

    pub fn button_height(&self) -> i32 {
        self.list_buttons.borrow().add.height()
    }

    pub fn list_buttons_pack(&self) -> &Pack {
        &self.list_buttons_pack
    }

    pub fn set_selected_row(&mut self, index: usize) {
        self.table.borrow_mut().set_selected(index);
    }

    pub fn open_editor(&mut self, index: usize, col: i32) {
        self.table.borrow_mut().open_editor(index, col);
    }
}

impl<T> ListEditor<T::DataType> for ListEditorTable<T>
where
    T: TableMapping + 'static,
    T::DataType: Clone,
{
    fn list_edited(&mut self, action: &ListAction<T::DataType>) {
        match action {
            ListAction::None => (),
            ListAction::Edit(index, value) => {
                self.table
                    .borrow_mut()
                    .edit_row(*index, |d| -> bool { T::edit_row(d, value) });
            }
            a => self.table.borrow_mut().edit_table(|table_vec| {
                process_list_action_map(table_vec, a, T::new_row, |row, new_value| {
                    T::edit_row(row, new_value);
                })
            }),
        }
    }

    fn clear_selected(&mut self) {
        self.table.borrow_mut().clear_selected();
    }

    fn set_selected(&mut self, index: usize, _: ItemId, _: &T::DataType) {
        self.table.borrow_mut().set_selected(index);
    }
}

impl<T> CompilerOutputGui<T::CompilerOutputType> for ListEditorTable<T>
where
    T: TableMapping + TableCompilerOutput + 'static,
{
    fn set_selected_compiler_output(&mut self, _: &Option<T::CompilerOutputType>) {}

    fn set_compiler_output(
        &mut self,
        index: usize,
        compiler_output: &Option<T::CompilerOutputType>,
    ) {
        self.table.borrow_mut().edit_row(index, |row| -> bool {
            T::set_row_state(row, compiler_output)
        });
    }
}
