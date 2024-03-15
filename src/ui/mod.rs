mod ime;
use std::{
    cmp::min,
    io::Cursor,
};
mod components;
use bevy::{
    prelude::*,
    render::render_resource::encase::rts_array::Length,
    utils::HashMap,
};
use bevy_egui::{
    egui::{
        self,
        epaint::CircleShape,
        ColorImage,
        ImageButton,
        Painter,
        Pos2,
        Response,
        Sense,
        Stroke,
        TextureHandle,
        TextureOptions,
        Vec2,
        Widget,
    },
    EguiContexts,
    EguiPlugin,
};
use bevy_persistent::Persistent;
use egui_extras::{
    Column,
    TableBuilder,
};
use image::{
    codecs::gif::GifDecoder,
    io::Reader,
    AnimationDecoder,
};
use ime::*;

use self::components::icon::Icon;
use crate::mirai::{
    MiraiIOSender,
    MiraiMessageChainType,
    MiraiMessageManager,
};

#[derive(Resource, Default)]
pub struct MyApp {
    single_text: String,
    multi_text: String,
}

pub struct UIPlugin;

#[derive(Resource)]
pub struct GIFImages {
    images: HashMap<String, Vec<(TextureHandle, u32)>>,
}

pub struct CircleImageButton {
    image: egui::TextureId,
    size: f32,
}

impl CircleImageButton {
    pub fn new(image: egui::TextureId, size: f32) -> Self { Self { image, size } }
}

impl Widget for CircleImageButton {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.size), Sense::click());
        let painter = Painter::new(ui.ctx().clone(), ui.layer_id(), rect);
        painter.add(egui::Shape::Circle(CircleShape {
            center: rect.center(),
            radius: self.size / 2.0,
            fill: Default::default(),
            stroke: Stroke::NONE,
        }));
        painter.add(egui::Shape::image(
            self.image,
            bevy_egui::egui::Rect::from_center_size(rect.center(), Vec2::splat(self.size)),
            bevy_egui::egui::Rect::from_center_size(rect.center(), Vec2::splat(self.size)),
            egui::Color32::WHITE,
        ));
        response
    }
}

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin)
            .add_plugins(ImePlugin)
            .insert_resource(MyApp::default())
            .add_systems(Startup, setup_system)
            .add_systems(
                Update,
                ui_system.run_if(resource_exists::<MiraiIOSender>),
            );
    }
}

pub fn setup_system(
    mut command: Commands,
    mut egui_context: EguiContexts,
    mut windows: Query<&mut Window>,
) {
    let ctx = egui_context.ctx_mut();
    let mut window = windows.single_mut();
    window.ime_enabled = true;
    let mut txt_font = egui::FontDefinitions::default();
    egui_extras::install_image_loaders(&ctx);
    txt_font
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Meiryo".to_owned());
    let fd = egui::FontData::from_static(include_bytes!(
        "C:/Windows/Fonts/simkai.ttf"
    ));
    txt_font.font_data.insert("Meiryo".to_owned(), fd);
    egui_context.ctx_mut().set_fonts(txt_font);
    command.insert_resource(GIFImages {
        images: HashMap::default(),
    })
}

pub fn ui_system(
    mut contexts: EguiContexts,
    mut app: ResMut<MyApp>,
    mut ime: ResMut<ImeManager>,
    sender: Res<MiraiIOSender>,
    time: Res<Time>,
    mut manager: ResMut<Persistent<MiraiMessageManager>>,
    mut gif_images: ResMut<GIFImages>,
) {
    let ctx = contexts.ctx_mut();
    let target_message_key = "1670426821";
    let mut heights: Vec<f32> = vec![];
    let willowbloosm_icon = egui::include_image!("../../assets/icons/willowbloosm.jpg");
    for message in manager.messages.get_mut(target_message_key).unwrap() {
        let mut height: f32 = 32.0;
        for chain in &message.data.message_chain {
            match &chain.variant {
                MiraiMessageChainType::Source(_) => {},
                MiraiMessageChainType::Image(image) => {
                    height += f32::min(image.height, 200.0);
                },
                _ => {
                    height += 16.0;
                },
            };
        }
        heights.push(height)
    }

    egui::SidePanel::left("party_panel")
        .exact_width(64.0)
        .show(ctx, |ui| {
            ui.allocate_ui(
                Vec2 { x: 48.0, y: 48.0 },
                |ui| {
                    ui.add(
                        Icon::new(willowbloosm_icon, Vec2 {
                            x: 48.0,
                            y: 48.0,
                        })
                        .rounding(48.0)
                        .uv([Pos2 { x: 0.0, y: 0.0 }, Pos2 { x: 1.0, y: 0.5 }]),
                    )
                }, /* egui::Button::image(willowbloosm_icon).
                    * rounding(40.0), */
            )
        });
    egui::CentralPanel::default().show(ctx, |ui| {
        egui::TopBottomPanel::top("top_panel")
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Top resizeable panel");
                ui.allocate_rect(
                    ui.available_rect_before_wrap(),
                    egui::Sense::hover(),
                );
            });
        egui::SidePanel::left("left_panel")
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Left resizeable panel");
                ui.allocate_rect(
                    ui.available_rect_before_wrap(),
                    egui::Sense::hover(),
                );
            });
        egui::SidePanel::right("right_panel")
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Right resizeable panel");
                ui.allocate_rect(
                    ui.available_rect_before_wrap(),
                    egui::Sense::hover(),
                );
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::TopBottomPanel::bottom("input_panel")
                .resizable(true)
                .show(ctx, |ui| {
                    ui.with_layout(
                        egui::Layout::bottom_up(egui::Align::Center),
                        |ui| {
                            let _teo_m = ime.text_edit_multiline(
                                &mut app.multi_text,
                                ui.max_rect().width(),
                                ui,
                                ctx,
                                sender.as_ref(),
                                &mut manager,
                            );
                        },
                    )
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .auto_shrink(false)
                    .cell_layout(egui::Layout::top_down(
                        egui::Align::LEFT,
                    ))
                    .stick_to_bottom(true)
                    .column(Column::remainder())
                    .min_scrolled_height(0.0)
                    .body(|body| {
                        body.heterogeneous_rows(heights.into_iter(), |mut row| {
                            let row_index = row.index();
                            let message =
                                &manager.messages.get_mut(target_message_key).unwrap()[row_index];
                            row.col(|ui| {
                                ui.label(message.data.sender.nickname.to_owned());
                                for chain in &message.data.message_chain {
                                    match &chain.variant {
                                        MiraiMessageChainType::Plain(plain) => {
                                            let text = format!("{}", plain.text);
                                            ui.add(egui::Label::new(text));
                                        },
                                        MiraiMessageChainType::Source(_) => {},
                                        MiraiMessageChainType::Image(image) => {
                                            if image.image_type == "GIF" {
                                                if !gif_images.images.contains_key(&image.image_id)
                                                {
                                                    let img_bytes = reqwest::blocking::get(
                                                        image.url.to_owned(),
                                                    )
                                                    .unwrap()
                                                    .bytes()
                                                    .unwrap();
                                                    let cursor = Cursor::new(img_bytes);
                                                    let decoder = GifDecoder::new(cursor).unwrap();

                                                    let frames = decoder
                                                        .into_frames()
                                                        .collect_frames()
                                                        .expect("Can't decode frames");
                                                    gif_images.images.insert(
                                                        image.image_id.to_owned(),
                                                        frames
                                                            .iter()
                                                            .enumerate()
                                                            .map(|(i, f)| {
                                                                let handle = ctx.load_texture(
                                                                format!("gif_frame_{i}"),
                                                                ColorImage::from_rgba_unmultiplied(
                                                                    [
                                                                        f.buffer().width() as _,
                                                                        f.buffer().height() as _,
                                                                    ],
                                                                    f.buffer(),
                                                                ),
                                                                TextureOptions::default(),
                                                            );
                                                                let (num, den) =
                                                                    f.delay().numer_denom_ms();
                                                                (
                                                                    handle,
                                                                    (num as f32 * 1000.0
                                                                        / den as f32)
                                                                        .round()
                                                                        as u32,
                                                                )
                                                            })
                                                            .collect(),
                                                    );
                                                }
                                                let images = gif_images
                                                    .images
                                                    .get(&image.image_id.to_owned())
                                                    .unwrap();
                                                let frame = ((time.elapsed_seconds()
                                                    / (images[0].1 as f32 / 500000.0))
                                                    as usize)
                                                    % images.len();
                                                ui.add(
                                                    egui::Image::new(&images[frame].0)
                                                        .max_size(bevy_egui::egui::Vec2 {
                                                            x: 400.0,
                                                            y: 200.0,
                                                        })
                                                        .fit_to_exact_size(bevy_egui::egui::Vec2 {
                                                            x: image.width,
                                                            y: image.height,
                                                        }),
                                                );
                                            } else {
                                                ui.add(
                                                    egui::Image::new(image.url.to_owned())
                                                        .max_size(bevy_egui::egui::Vec2 {
                                                            x: 400.0,
                                                            y: 200.0,
                                                        })
                                                        .fit_to_exact_size(bevy_egui::egui::Vec2 {
                                                            x: image.width,
                                                            y: image.height,
                                                        }),
                                                );
                                            }
                                        },
                                    }
                                }
                            });
                        })
                    });
            });
        });
    });
}
