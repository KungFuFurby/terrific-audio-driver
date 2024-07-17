//! Sound Effect Export Order

// SPDX-FileCopyrightText: © 2024 Marcus Rowe <undisbeliever@gmail.com>
//
// SPDX-License-Identifier: MIT

use std::ops::Range;

use crate::list_editor::{
    LaVec, ListAction, ListEditorTable, ListMessage, TableAction, TableMapping,
};
use crate::names::{deduplicate_item_name, deduplicate_names};
use crate::tables::{SimpleRow, TableEvent};
use crate::GuiMessage;

use compiler::data::Name;
use compiler::driver_constants::MAX_SOUND_EFFECTS;
use compiler::sound_effects::SfxExportOrder;

use fltk::app::Sender;
use fltk::group::Flex;

#[derive(Debug)]
pub struct SfxId(u8);

impl SfxId {
    pub fn value(&self) -> u8 {
        self.0
    }
}

#[derive(Debug)]
pub struct SfxExportOrderAction {
    action: ListAction<Name>,
    low_priority_index: usize,
}

#[derive(Debug, Clone)]
pub struct GuiSfxExportOrder {
    export_order: LaVec<Name>,
    low_priority_index: usize,
}

impl Default for GuiSfxExportOrder {
    fn default() -> Self {
        Self {
            low_priority_index: 0,
            export_order: LaVec::new(),
        }
    }
}

impl GuiSfxExportOrder {
    pub fn new_lossy(eo: Vec<Name>, lp_eo: Vec<Name>) -> (Self, usize) {
        let low_priority_index = eo.len();

        let export_order = [eo, lp_eo].concat();
        let (export_order, n_renamed) = deduplicate_names(export_order);

        (
            Self {
                export_order: LaVec::from_vec(export_order.into_vec()),
                low_priority_index,
            },
            n_renamed,
        )
    }

    pub fn process(&mut self, action: &SfxExportOrderAction) {
        self.export_order.process(&action.action);
        self.low_priority_index = action.low_priority_index;
    }

    pub fn can_add_one(&self) -> bool {
        self.export_order.len() < MAX_SOUND_EFFECTS
    }

    pub fn normal_priority_sfx(&self) -> &[Name] {
        &self.export_order[0..self.low_priority_index]
    }

    pub fn low_priority_sfx(&self) -> &[Name] {
        &self.export_order[self.low_priority_index..]
    }

    fn table_max_sizes(&self) -> (usize, usize) {
        (
            MAX_SOUND_EFFECTS.saturating_sub(self.low_priority_sfx().len()),
            MAX_SOUND_EFFECTS.saturating_sub(self.normal_priority_sfx().len()),
        )
    }
}

impl SfxExportOrder for GuiSfxExportOrder {
    fn n_sound_effects(&self) -> usize {
        self.export_order.len()
    }

    fn export_order(&self) -> &[Name] {
        &self.export_order
    }

    fn low_priority_index(&self) -> usize {
        self.low_priority_index
    }
}

impl GuiSfxExportOrder {
    pub fn sfx_id(&self, index: usize) -> Option<SfxId> {
        if index < self.export_order.len() {
            index.try_into().ok().map(SfxId)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub enum SfxExportOrderMessage {
    NormalPriority(ListMessage<Name>),
    LowPriority(ListMessage<Name>),
}

pub trait SfxEoMapping {
    const TYPE_NAME: &'static str;
    const HEADER: &'static str;

    fn to_gui_message(lm: ListMessage<Name>) -> GuiMessage {
        GuiMessage::EditSfxExportOrder(SfxExportOrderMessage::NormalPriority(lm))
    }
}

struct NormalSfxEoMapping;
impl SfxEoMapping for NormalSfxEoMapping {
    const TYPE_NAME: &'static str = "sound effect";
    const HEADER: &'static str = "Sound Effect Export Order";

    fn to_gui_message(lm: ListMessage<Name>) -> GuiMessage {
        GuiMessage::EditSfxExportOrder(SfxExportOrderMessage::NormalPriority(lm))
    }
}

struct LowPrioritySfxEoMapping;
impl SfxEoMapping for LowPrioritySfxEoMapping {
    const TYPE_NAME: &'static str = "sound effect";
    const HEADER: &'static str = "Low Priority SFX Export Order";

    fn to_gui_message(lm: ListMessage<Name>) -> GuiMessage {
        GuiMessage::EditSfxExportOrder(SfxExportOrderMessage::LowPriority(lm))
    }
}

impl<T> TableMapping for T
where
    T: SfxEoMapping,
{
    type DataType = Name;
    type RowType = SimpleRow<1>;

    const CAN_CLONE: bool = true;
    const CAN_EDIT: bool = true;

    fn type_name() -> &'static str {
        T::TYPE_NAME
    }

    fn headers() -> Vec<String> {
        vec![T::HEADER.to_owned()]
    }

    fn to_message(lm: ListMessage<Name>) -> GuiMessage {
        T::to_gui_message(lm)
    }

    fn add_clicked() -> GuiMessage {
        Self::to_gui_message(ListMessage::Add("name".to_owned().try_into().unwrap()))
    }

    fn new_row(sfx_name: &Name) -> Self::RowType {
        SimpleRow::new([sfx_name.as_str().to_string()])
    }

    fn edit_row(r: &mut Self::RowType, sfx_name: &Name) -> bool {
        r.edit_column(0, sfx_name.as_str())
    }

    fn table_event(event: TableEvent, _row: usize, _col: i32) -> TableAction {
        match event {
            TableEvent::Enter | TableEvent::EditorRequested | TableEvent::CellClicked => {
                TableAction::OpenEditor
            }
            TableEvent::DoubleClick => TableAction::None,
        }
    }

    fn commit_edited_value(index: usize, col: i32, value: String) -> Option<GuiMessage> {
        match col {
            0 => Name::try_new_lossy(value)
                .map(|name| T::to_gui_message(ListMessage::ItemEdited(index, name))),
            _ => None,
        }
    }
}

pub struct SfxExportOrderEditor {
    normal_priority: ListEditorTable<NormalSfxEoMapping>,
    low_priority: ListEditorTable<LowPrioritySfxEoMapping>,
}

impl SfxExportOrderEditor {
    pub fn new(
        parent: &mut Flex,
        sfx_export_order: &GuiSfxExportOrder,
        sender: Sender<GuiMessage>,
    ) -> Self {
        let (max_normal, max_lp) = sfx_export_order.table_max_sizes();

        // ::TODO add a button to move SFX between low and high priorities::
        let normal_priority = ListEditorTable::new_from_slice(
            parent,
            sfx_export_order.normal_priority_sfx(),
            max_normal,
            sender.clone(),
        );
        let low_priority = ListEditorTable::new_from_slice(
            parent,
            sfx_export_order.low_priority_sfx(),
            max_lp,
            sender.clone(),
        );

        Self {
            normal_priority,
            low_priority,
        }
    }

    fn process_move<T>(
        table: &mut ListEditorTable<T>,
        range: Range<usize>,
        from: usize,
        to: usize,
    ) -> Option<(ListAction<Name>, isize)>
    where
        T: SfxEoMapping,
    {
        assert_ne!(from, to);

        let eo_offset = range.start;

        table.sfx_eo_edited(&ListAction::Move(from, to));
        table.set_selected_row(to);

        Some((ListAction::Move(from + eo_offset, to + range.start), 0))
    }

    fn process_list_message<T>(
        m: ListMessage<Name>,
        table: &mut ListEditorTable<T>,
        data: &GuiSfxExportOrder,
        range: Range<usize>,
    ) -> Option<(ListAction<Name>, isize)>
    where
        T: SfxEoMapping,
    {
        let eo_offset = range.start;
        let slice = &data.export_order[range.clone()];

        let deduplicate_name =
            |name, index| match deduplicate_item_name(&name, data.export_order(), index) {
                Some(n) => n,
                None => name,
            };

        match m {
            ListMessage::ItemEdited(index, new_name) => match slice.get(index) {
                Some(name) if name != &new_name => {
                    let new_name = deduplicate_name(new_name, Some(index));

                    table.sfx_eo_edited(&ListAction::Edit(index, new_name.clone()));

                    Some((ListAction::Edit(index + eo_offset, new_name), 0))
                }
                _ => None,
            },

            ListMessage::Add(name) | ListMessage::AddWithItemId(_, name) => {
                if data.can_add_one() {
                    let i = slice.len();
                    let name = deduplicate_name(name, None);

                    table.sfx_eo_edited(&ListAction::Add(i, name.clone()));
                    table.set_selected_row(i);

                    table.open_editor(i, 0);

                    Some((ListAction::Add(i + eo_offset, name), 1))
                } else {
                    None
                }
            }

            ListMessage::Clone(index) => match (slice.get(index), data.can_add_one()) {
                (Some(name), true) => {
                    let i = index + 1;
                    let name = deduplicate_name(name.clone(), None);

                    table.sfx_eo_edited(&ListAction::Add(i, name.clone()));
                    table.set_selected_row(i);

                    Some((ListAction::Add(i + eo_offset, name), 1))
                }
                _ => None,
            },

            ListMessage::Remove(index) => {
                if index < slice.len() {
                    table.sfx_eo_edited(&ListAction::Remove(index));
                    table.set_selected_row(index);

                    Some((ListAction::Remove(index + eo_offset), -1))
                } else {
                    None
                }
            }
            ListMessage::MoveToTop(index) => {
                if index > 0 && index < slice.len() {
                    Self::process_move(table, range, index, 0)
                } else {
                    None
                }
            }
            ListMessage::MoveUp(index) => {
                if index > 0 && index < slice.len() {
                    Self::process_move(table, range, index, index - 1)
                } else {
                    None
                }
            }
            ListMessage::MoveDown(index) => {
                if index + 1 < slice.len() {
                    Self::process_move(table, range, index, index + 1)
                } else {
                    None
                }
            }
            ListMessage::MoveToBottom(index) => {
                if index + 1 < slice.len() {
                    Self::process_move(table, range, index, slice.len() - 1)
                } else {
                    None
                }
            }

            // Not supported
            ListMessage::AddMultiple(..) => None,
        }
    }

    pub fn process(
        &mut self,
        m: SfxExportOrderMessage,
        data: &mut GuiSfxExportOrder,
    ) -> Option<SfxExportOrderAction> {
        let a = match m {
            SfxExportOrderMessage::NormalPriority(m) => {
                self.low_priority.clear_selected_row();

                let (action, size_delta) = Self::process_list_message(
                    m,
                    &mut self.normal_priority,
                    data,
                    0..data.low_priority_index,
                )?;

                SfxExportOrderAction {
                    action,
                    low_priority_index: data
                        .low_priority_index
                        .checked_add_signed(size_delta)
                        .unwrap(),
                }
            }
            SfxExportOrderMessage::LowPriority(m) => {
                self.normal_priority.clear_selected_row();

                let (action, _size_delta) = Self::process_list_message(
                    m,
                    &mut self.low_priority,
                    data,
                    data.low_priority_index..data.export_order.len(),
                )?;

                SfxExportOrderAction {
                    action,
                    low_priority_index: data.low_priority_index,
                }
            }
        };

        data.process(&a);

        let (max_normal, max_lp) = data.table_max_sizes();
        self.normal_priority.set_max_size(max_normal);
        self.low_priority.set_max_size(max_lp);

        Some(a)
    }
}