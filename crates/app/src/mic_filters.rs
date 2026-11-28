//! Présentation commune des filtres du micro GoXLR et des entrées Windows.
use crate::{mixer::Mixer, ui::{self, Icon, alpha, rgb}};
use gpui::{Context, Div, FontWeight, IntoElement, div, prelude::*, px};
use rustudio_core::{consts::RATIOS, dsp::Comp, state::MicCal};
use std::time::Instant;

fn compressed_level(input: f64, comp: &Comp) -> f64 {
    let over = input - f64::from(comp.threshold);
    let excess = if over <= -3. { 0. } else if over >= 3. { over } else { (over + 3.).powi(2) / 12. };
    let ratio = RATIOS[comp.ratio.clamp(0, (RATIOS.len() - 1) as i32) as usize];
    input - excess * (1. - 1. / ratio) + f64::from(comp.makeup)
}

/// Réduit le gain ajouté et la compression sans perdre plus de 3 dB sur la voix mesurée.
pub(crate) fn softer_compressor(current: &Comp, cal: &MicCal, gain: i32) -> Option<Comp> {
    if current.ratio == 0 || (current.ratio <= 7 && current.makeup <= 2) {
        return None;
    }
    let shift = f64::from(gain) - f64::from(cal.gain_db);
    let voice = cal.voice_db + shift;
    let peak = cal.loud_peak_db.unwrap_or(cal.voice_peak_db).max(cal.voice_peak_db) + shift;
    if !voice.is_finite() || !peak.is_finite() || peak < voice {
        return None;
    }
    let (voice, peak) = (voice.min(0.), peak.min(0.));
    let goal = compressed_level(voice, current);
    let preferred_threshold = (voice + 2.).round().clamp(-40., 0.) as i32;
    let mut next = Comp {
        threshold: preferred_threshold,
        ratio: current.ratio.min(7),
        attack: current.attack,
        release: current.release.max(11),
        makeup: 0,
    };
    let mut best = None;
    let mut best_cost = f64::INFINITY;
    for threshold in -40..=0 {
        next.threshold = threshold;
        next.makeup = 0;
        let headroom = (-3. - compressed_level(peak, &next)).floor() as i32;
        let max_makeup = headroom.min(current.makeup).min(2);
        if max_makeup < -6 { continue; }
        next.makeup = ((goal - compressed_level(voice, &next)).round() as i32).clamp(-6, max_makeup);
        let error = (goal - compressed_level(voice, &next)).abs();
        if error > 3. { continue; }
        let cost = error + 0.08 * f64::from((threshold - preferred_threshold).abs());
        if cost < best_cost {
            best_cost = cost;
            best = Some(next.clone());
        }
    }
    best
}

pub(crate) fn mic_filter_columns(width: f32) -> usize {
    ui::columns_that_fit(width, 340., 10., 2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MicFilter {
    Noise,
    HighPass,
    Gate,
    Eq,
    Compressor,
    Deesser,
    Limiter,
}

#[derive(Default)]
pub(crate) struct FilterPanels {
    selected: Option<MicFilter>,
    panels: [ui::Disclosure; MicFilter::Limiter as usize + 1],
}

impl FilterPanels {
    pub(crate) fn select(&mut self, next: Option<MicFilter>, now: Instant) {
        if self.selected == next { return; }
        if let Some(previous) = self.selected { self.panels[previous as usize].set(false, now); }
        if let Some(next) = next { self.panels[next as usize].set(true, now); }
        self.selected = next;
    }
}

impl MicFilter {
    fn presentation(self) -> (Icon, &'static str, &'static str) {
        match self {
            Self::Noise => (Icon::Sparkles, "Suppression du bruit (IA)", "Atténue le bruit pendant que tu parles."),
            Self::HighPass => (Icon::Wave, "Coupe-bas", "Atténue les grondements."),
            Self::Gate => (Icon::MicOff, "Noise gate", "Atténue le bruit entre tes phrases."),
            Self::Eq => (Icon::Activity, "Égaliseur", "Ajuste les graves, les médiums et les aigus."),
            Self::Compressor => (Icon::Gauge, "Compresseur", "Réduit les écarts de volume."),
            Self::Deesser => (Icon::Sliders, "De-esser", "Adoucit les sifflantes."),
            Self::Limiter => (Icon::Shield, "Limiteur", "Retient les pics de volume."),
        }
    }
}

impl Mixer {
    /// L’intitulé ouvre les réglages. L’action à droite est indépendante.
    pub(crate) fn mic_filter_row(
        &self,
        filter: MicFilter,
        width: f32,
        expandable: bool,
        action: impl IntoElement,
        settings: impl FnOnce(&mut Context<Self>) -> Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let (glyph, title, description) = filter.presentation();
        let spacious = width >= 900.;
        let (title_size, detail_size, vertical_padding) =
            if spacious { (18., 14., 15.) } else { (16., 13., 11.) };
        let panel = &self.mic_filters.panels[filter as usize];
        let open = expandable && panel.is_open();
        let label = div()
            .id(gpui::ElementId::NamedInteger("mic-filter".into(), filter as u64))
            .flex_1()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(px(12.))
            .py(px(vertical_padding))
            .when(expandable, |d| {
                d.cursor(gpui::CursorStyle::PointingHand)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.mic_filters.select((this.mic_filters.selected != Some(filter)).then_some(filter), Instant::now());
                        cx.notify();
                    }))
            })
            .child(ui::icon(glyph, if spacious { 20. } else { 17. }, rgb(if open { ui::ACCENT } else { ui::TEXT2 })).flex_none())
            .child(div().min_w(px(0.)).flex_1().flex().flex_col().gap(px(2.))
                .child(div().text_size(px(title_size)).line_height(px(title_size + 5.))
                    .font_weight(FontWeight::SEMIBOLD).child(title))
                .child(ui::caption(description).text_size(px(detail_size)).line_height(px(detail_size + 4.))))
            .child(div().w(px(18.)).flex_none().when(expandable, |d| {
                d.child(ui::icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }, 15., rgb(ui::TEXT2)))
            }));

        div()
            .w_full()
            .min_w(px(0.))
            .border_b_1()
            .border_color(alpha(ui::INK, 0.08))
            .child(div().flex().items_center().gap(px(18.)).child(label).child(div().flex_none().child(action)))
            .when(expandable, |d| d.children(panel.body(|| div().min_w(px(0.)).pb(px(18.)).child(settings(cx)))))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MicTab {
    #[default]
    Input,
    Filters,
    Presets,
    Listen,
}

impl MicTab {
    pub(crate) const ALL: [(Self, Icon, &'static str); 4] = [
        (Self::Input, Icon::Mic, "Entrée"),
        (Self::Filters, Icon::Sliders, "Filtres"),
        (Self::Presets, Icon::Wave, "Préréglages"),
        (Self::Listen, Icon::Headphones, "Écoute"),
    ];
}

/// Le niveau est un repère du gain. Il reste lisible à côté de sa valeur.
pub(crate) fn mic_signal(shown: Option<f64>, measured: Option<f64>) -> Div {
    let fraction = shown.map_or(0., |db| ((db + 72.) / 72.).clamp(0., 1.) as f32);
    let colour = if measured.is_some_and(|db| db >= -6.) { ui::ORANGE } else { ui::ACCENT };
    let value = measured.map_or_else(|| "En attente".into(), |db| format!("{db:.0} dB").replace('-', "−"));
    div().flex().flex_none().items_center().gap(px(8.))
        .child(ui::caption("Niveau"))
        .child(div().w(px(116.)).child(ui::progress(fraction, colour).overflow_hidden()))
        .child(ui::caption(value).w(px(54.)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustudio_core::{dsp, micchain, micdsp::{BLOCK, Chain, FULL}};

    #[test]
    fn les_reglages_du_filtre_s_empilent_a_la_largeur_minimale() {
        assert_eq!(mic_filter_columns(662.), 1);
        assert_eq!(mic_filter_columns(970.), 2);
        assert_eq!(mic_filter_columns(1160.), 2);
    }

    #[test]
    fn un_seul_panneau_reste_ouvert_apres_des_clics_rapides() {
        let now = Instant::now();
        let mut filters = FilterPanels::default();
        filters.select(Some(MicFilter::Gate), now);
        filters.select(Some(MicFilter::Eq), now + std::time::Duration::from_millis(70));
        filters.select(Some(MicFilter::Gate), now + std::time::Duration::from_millis(110));
        assert!(filters.panels[MicFilter::Gate as usize].is_open());
        assert!(!filters.panels[MicFilter::Eq as usize].is_open());
        filters.select(None, now + std::time::Duration::from_millis(180));
        assert!(filters.panels.iter().all(|p| !p.is_open()));
    }

    #[test]
    fn la_compression_douce_reduit_le_souffle_sans_deplacer_le_niveau_typique() {
        let current = Comp { threshold: -31, ratio: 9, attack: 2, release: 10, makeup: 8 };
        let cal = MicCal {
            kind: "condenser".into(), gain_db: 16, t: 0., noise_db: -54., noise_peak_db: -49.,
            voice_low_db: -34., voice_db: -24., voice_peak_db: -10., loud_peak_db: Some(-4.),
        };
        let soft = softer_compressor(&current, &cal, 16).expect("réglage compatible avec les mesures");
        assert!(soft.makeup <= 2 && soft.ratio <= 7);
        assert!((compressed_level(cal.voice_db, &soft) - compressed_level(cal.voice_db, &current)).abs() <= 1.5);
        assert!(compressed_level(-4., &soft) <= -3.);
        assert!(softer_compressor(&soft, &cal, 16).is_none());

        let run = |comp: &Comp, level: f64| {
            let mut d = dsp::preset_dsp("brut");
            d.comp = comp.clone();
            let params = micchain::design(&d);
            let mut chain = Chain::default();
            let amplitude = (10_f64.powf(level / 20.) * f64::from(FULL)) as i32;
            let mut peak = 0;
            for _ in 0..2000 {
                let mut signal: [i32; BLOCK] = std::array::from_fn(|i| if i % 2 == 0 { amplitude } else { -amplitude });
                chain.process(&params, &mut signal);
                peak = signal.iter().map(|x| x.abs()).max().unwrap();
            }
            peak
        };
        assert!(run(&soft, -50.) < run(&current, -50.) * 6 / 10);
        let voice = f64::from(run(&soft, -24.)) / f64::from(run(&current, -24.));
        assert!((20. * voice.log10()).abs() <= 1.5);
        let swing = |comp| 14. + 20. * (f64::from(run(comp, -28.)) / f64::from(run(comp, -14.))).log10();
        assert!(swing(&soft) < swing(&current) - 3., "le fond doit moins varier au changement de volume");
    }
}
