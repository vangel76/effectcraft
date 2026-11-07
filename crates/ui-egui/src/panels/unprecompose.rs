//! Layer ▸ Un-precompose from the menu: when expanding the pre-comp layer would change the picture
//! or drop something (the command's dry run lists why), the reasons are shown first and the user
//! chooses to cancel or to unpack anyway (`force`). Cancel changes nothing; a pre-comp layer
//! without reasons is unpacked at once, and one that cannot be unpacked at all (locked, not a
//! pre-comp) reports its error without the prompt. Agents running `layer.unprecompose` with
//! `engine.execute` are never asked: they pass `force` or get the error listing the reasons.
//!
//! Automation ids: `dialog.unprecompose.force`, `dialog.unprecompose.cancel`.

use crate::i18n::tr;
use egui::vec2;
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{Dialog, EffectcraftApp};

const COMMAND: &str = "layer.unprecompose";

/// The unpack waiting on the prompt: its params and the dry run's reasons.
#[derive(Clone, Debug, Default)]
pub struct Pending {
    pub command: Option<(Value, Vec<String>)>,
}

/// Whether command `id` with `params` needs the prompt first. When it does, the prompt opens
/// holding the command and the caller must not run it. `Err` when the dry run fails (a locked
/// layer, a layer that is not a pre-comp, several layers), with the message.
pub fn guard(app: &mut EffectcraftApp, id: &str, params: &Value) -> Result<bool, String> {
    if id != COMMAND || params.get("force") == Some(&Value::Bool(true)) || params.get("dryRun") == Some(&Value::Bool(true)) {
        return Ok(false);
    }
    let mut dry = params.as_object().cloned().unwrap_or_default();
    dry.insert("dryRun".into(), Value::Bool(true));
    let r = app.session.execute(COMMAND, Value::Object(dry)).map_err(|e| e.to_string())?;
    let warnings: Vec<String> = r["warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    if warnings.is_empty() {
        return Ok(false);
    }
    // The layer the warnings are about, by the ids the dry run resolved, whatever is selected,
    // reordered or renamed by the time the prompt is answered.
    let (Some(comp), Some(layer)) = (r["comp"].as_u64(), r["layer"].as_u64()) else { return Ok(false) };
    let mut pinned = params.as_object().cloned().unwrap_or_default();
    pinned.remove("layer");
    pinned.insert("layers".into(), json!([layer]));
    pinned.insert("comp".into(), json!(comp));
    app.dialog_state.unprecompose.command = Some((Value::Object(pinned), warnings));
    app.dialog = Some(Dialog::Unprecompose);
    Ok(true)
}

pub fn show(app: &mut EffectcraftApp, ctx: &egui::Context, t: &Tokens) {
    let Some((_, warnings)) = app.dialog_state.unprecompose.command.clone() else {
        app.dialog = None;
        return;
    };
    let mut answer: Option<bool> = None;
    super::dialogs::modal_vertical(ctx, "Un-precompose", vec2(520.0, 300.0), t, |ui| {
        ui.label(egui::RichText::new(tr("Unpacking this pre-comp layer changes the picture or drops something:")).color(t.tab_text_active));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
            for w in &warnings {
                ui.label(egui::RichText::new(format!("•  {w}")).color(t.tab_text_active));
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tr("What the pre-comp layer itself carries is dropped. You can undo this.")).color(t.text_dim).size(11.0));
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = tr("Unpack Anyway");
            let r = ui.button(label);
            app.auto.add("dialog.unprecompose.force", r.rect, label);
            if r.clicked() {
                answer = Some(true);
            }
            let r = ui.button(tr("Cancel"));
            app.auto.add("dialog.unprecompose.cancel", r.rect, "Cancel");
            if r.clicked() {
                answer = Some(false);
            }
        });
    });
    if answer.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(false);
    }
    if let Some(go) = answer {
        finish(app, ctx, go);
    }
}

/// The prompt's answer: Cancel (`false`) drops the unpack, Unpack Anyway runs it with `force`.
fn finish(app: &mut EffectcraftApp, ctx: &egui::Context, go: bool) {
    app.dialog = None;
    let Some((params, _)) = app.dialog_state.unprecompose.command.take() else { return };
    if !go {
        return;
    }
    // A layer id that is gone would be read as a layer number: unpack only the layer warned about.
    if !still_there(&app.session, &params) {
        app.ui.status = tr("The pre-comp layer is no longer there; nothing was unpacked.").to_string();
        return;
    }
    // An error is already in the status bar.
    let _ = crate::menus::run_engine(app, ctx, COMMAND, forced(&params));
}

/// Whether the pinned comp still holds the pinned layer (by id).
fn still_there(s: &effectcraft_engine::Session, params: &Value) -> bool {
    let comp = params["comp"].as_u64().map(effectcraft_project::ItemId);
    let layer = params["layers"].get(0).and_then(Value::as_u64).map(effectcraft_project::LayerId);
    comp.zip(layer).is_some_and(|(c, l)| s.project.comp(c).is_some_and(|c| c.layer(l).is_some()))
}

/// `params` with the user's consent to the reasons the prompt listed.
fn forced(params: &Value) -> Value {
    let mut p = params.as_object().cloned().unwrap_or_default();
    p.insert("force".into(), json!(true));
    Value::Object(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use effectcraft_engine::Session;

    /// A comp with two solids nested into a pre-comp; the pre-comp layer's id.
    fn scene(app: &mut EffectcraftApp) -> u64 {
        let s = &mut app.session;
        s.execute("comp.new", json!({"name": "Main", "width": 640, "height": 360, "duration": 4.0, "fps": 24.0})).unwrap();
        let a = s.execute("layer.newSolid", json!({"color": "#ff0000"})).unwrap()["layer"].as_u64().unwrap();
        let b = s.execute("layer.newSolid", json!({"color": "#00ff00"})).unwrap()["layer"].as_u64().unwrap();
        s.execute("layer.precompose", json!({"layers": [a, b], "name": "Inner"})).unwrap()["layer"].as_u64().unwrap()
    }

    fn layer_count(s: &Session) -> usize {
        s.active_comp().map_or(0, |c| c.layers.len())
    }

    fn fresh() -> (egui::Context, EffectcraftApp) {
        let ctx = egui::Context::default();
        let app = EffectcraftApp::new(Session::default());
        crate::theme::install(&ctx, &app.tokens, crate::i18n::language(&app));
        (ctx, app)
    }

    #[test]
    fn the_menu_asks_and_unpack_anyway_unpacks() {
        let (ctx, mut app) = fresh();
        let pre = scene(&mut app);
        app.session.execute("prop.set", json!({"layer": pre, "path": "transform/opacity", "value": 50})).unwrap();
        let params = json!({"layers": [pre]});
        let r = crate::menus::invoke(&mut app, &ctx, COMMAND, params.clone()).unwrap();
        assert_eq!(r, json!({"dialog": "unprecompose"}));
        assert_eq!(app.dialog, Some(Dialog::Unprecompose));
        let (_, warnings) = app.dialog_state.unprecompose.command.clone().unwrap();
        assert!(warnings.iter().any(|w| w.contains("Opacity")), "{warnings:?}");
        // Drawn, the prompt offers both answers under their automation ids.
        crate::panels::dialogs::press_key(&mut app, &ctx, egui::Key::A, egui::Modifiers::NONE);
        assert!(app.auto.find("dialog.unprecompose.force").is_some() && app.auto.find("dialog.unprecompose.cancel").is_some());
        // Escape is Cancel: nothing changes.
        crate::panels::dialogs::press_key(&mut app, &ctx, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(app.dialog.is_none() && app.dialog_state.unprecompose.command.is_none());
        assert_eq!(layer_count(&app.session), 1, "Cancel left the pre-comp layer");
        // Unpack Anyway unpacks.
        crate::menus::invoke(&mut app, &ctx, COMMAND, params).unwrap();
        assert_eq!(app.dialog, Some(Dialog::Unprecompose));
        finish(&mut app, &ctx, true);
        assert!(app.dialog.is_none());
        assert_eq!(layer_count(&app.session), 2, "the two solids are back");
    }

    /// A window-sized frame's input with `events`.
    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 860.0))), events, ..Default::default() }
    }

    /// Draw a frame with the open dialog.
    fn frame(app: &mut EffectcraftApp, ctx: &egui::Context) {
        ctx.run_ui(input(vec![]), |ui| {
            app.auto.begin_frame();
            crate::panels::dialogs::show(app, ui.ctx());
        })
        .textures_delta
        .clear();
    }

    /// Click the widget registered as `id`, then draw the frames that follow.
    fn click(app: &mut EffectcraftApp, ctx: &egui::Context, id: &str) {
        // The prompt settles (sized, then centred) before it is clicked.
        for _ in 0..4 {
            frame(app, ctx);
        }
        let [x, y, w, h] = app.auto.find(id).unwrap_or_else(|| panic!("{id} is not on screen")).rect;
        let pos = egui::pos2(x + w / 2.0, y + h / 2.0);
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for event in [egui::Event::PointerMoved(pos), button(true), button(false)] {
            ctx.run_ui(input(vec![event]), |ui| {
                app.auto.begin_frame();
                crate::panels::dialogs::show(app, ui.ctx());
            })
            .textures_delta
            .clear();
        }
        frame(app, ctx);
    }

    /// The prompt's buttons, clicked: Cancel keeps the pre-comp layer, Unpack Anyway unpacks it.
    /// The question itself (the dry run) leaves no journal entry; the unpack leaves one.
    #[test]
    fn the_prompts_buttons_cancel_and_unpack_and_asking_is_not_journaled() {
        let (ctx, mut app) = fresh();
        let pre = scene(&mut app);
        app.session.execute("prop.set", json!({"layer": pre, "path": "transform/opacity", "value": 50})).unwrap();
        let journal = app.session.journal.len();
        crate::menus::invoke(&mut app, &ctx, COMMAND, json!({"layers": [pre]})).unwrap();
        assert_eq!(app.session.journal.len(), journal, "asking is not journaled");
        frame(&mut app, &ctx);
        frame(&mut app, &ctx);
        click(&mut app, &ctx, "dialog.unprecompose.cancel");
        assert!(app.dialog.is_none() && app.dialog_state.unprecompose.command.is_none());
        assert_eq!(layer_count(&app.session), 1, "Cancel left the pre-comp layer");
        assert_eq!(app.session.journal.len(), journal, "Cancel leaves no trace in the journal");
        crate::menus::invoke(&mut app, &ctx, COMMAND, json!({"layers": [pre]})).unwrap();
        frame(&mut app, &ctx);
        frame(&mut app, &ctx);
        click(&mut app, &ctx, "dialog.unprecompose.force");
        assert!(app.dialog.is_none());
        assert_eq!(layer_count(&app.session), 2, "Unpack Anyway brought the two solids back");
        assert_eq!(app.session.journal.last().map(|j| j.0.as_str()), Some(COMMAND));
        assert_eq!(app.session.journal.len(), journal + 1);
    }

    /// The prompt unpacks the layer it warned about, by id: named by its number it stays that
    /// layer when the order changes, and once it is gone Unpack Anyway unpacks nothing.
    #[test]
    fn unpack_anyway_keeps_to_the_layer_it_warned_about() {
        let (ctx, mut app) = fresh();
        let pre = scene(&mut app);
        app.session.execute("prop.set", json!({"layer": pre, "path": "transform/opacity", "value": 50})).unwrap();
        crate::menus::invoke(&mut app, &ctx, COMMAND, json!({"layer": "#1"})).unwrap();
        let (params, _) = app.dialog_state.unprecompose.command.clone().unwrap();
        assert_eq!(params["layers"], json!([pre]), "{params}");
        assert!(params.get("layer").is_none(), "{params}");
        app.session.execute("edit.clear", json!({"layers": [pre]})).unwrap();
        let before = app.session.project.clone();
        finish(&mut app, &ctx, true);
        assert!(app.dialog.is_none());
        assert!(app.ui.status.contains("no longer there"), "{}", app.ui.status);
        assert!(std::sync::Arc::ptr_eq(&app.session.project, &before), "nothing else was unpacked");
    }

    #[test]
    fn a_clean_precomp_unpacks_without_asking_and_a_locked_one_is_an_error() {
        let (ctx, mut app) = fresh();
        let pre = scene(&mut app);
        assert_eq!(guard(&mut app, COMMAND, &json!({"layers": [pre]})), Ok(false));
        crate::menus::invoke(&mut app, &ctx, COMMAND, json!({"layers": [pre]})).unwrap();
        assert!(app.dialog.is_none());
        assert_eq!(layer_count(&app.session), 2);

        let (_, mut app) = fresh();
        let pre = scene(&mut app);
        app.session.execute("prop.set", json!({"layer": pre, "path": "transform/opacity", "value": 50})).unwrap();
        app.session.execute("layer.setSwitch", json!({"layers": [pre], "switch": "lock", "value": true})).unwrap();
        let e = guard(&mut app, COMMAND, &json!({"layers": [pre]})).unwrap_err();
        assert!(e.contains("locked"), "{e}");
        assert!(app.dialog.is_none());
        assert_eq!(layer_count(&app.session), 1);
        // Consented and dry-run calls, and other commands, are not asked.
        assert_eq!(guard(&mut app, COMMAND, &json!({"layers": [pre], "force": true})), Ok(false));
        assert_eq!(guard(&mut app, COMMAND, &json!({"layers": [pre], "dryRun": true})), Ok(false));
        assert_eq!(guard(&mut app, "layer.precompose", &json!({})), Ok(false));
    }
}
