//! Liste des peripheriques et regles de validation (pures, testables sans carte son).

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, SampleFormat};

/// Frequence imposee aux peripheriques (H5).
pub const REQUIRED_RATE: u32 = 48_000;

pub struct Entry {
    pub name: String,
    pub device: Device,
}

fn device_name(device: &Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| device.to_string())
}

/// Le peripherique "par defaut" de cpal est un handle special : on compare les identifiants.
fn same_device(a: &Device, b: &Device) -> bool {
    matches!((a.id(), b.id()), (Ok(x), Ok(y)) if x == y)
}

pub fn list_inputs(host: &cpal::Host) -> (Vec<Entry>, Option<usize>) {
    let default = host.default_input_device();
    let entries: Vec<Entry> = host
        .input_devices()
        .map(|it| it.map(|d| Entry { name: device_name(&d), device: d }).collect())
        .unwrap_or_default();
    let idx = default.and_then(|d| entries.iter().position(|e| same_device(&e.device, &d)));
    (entries, idx)
}

pub fn list_outputs(host: &cpal::Host) -> (Vec<Entry>, Option<usize>) {
    let default = host.default_output_device();
    let entries: Vec<Entry> = host
        .output_devices()
        .map(|it| it.map(|d| Entry { name: device_name(&d), device: d }).collect())
        .unwrap_or_default();
    let idx = default.and_then(|d| entries.iter().position(|e| same_device(&e.device, &d)));
    (entries, idx)
}

/// Affiche les peripheriques et leur format natif (diagnostic : `clearmic-app --list-devices`).
pub fn print_report() {
    let host = cpal::default_host();
    let (inputs, default_in) = list_inputs(&host);
    let (outputs, default_out) = list_outputs(&host);
    for (title, entries, default, input) in [
        ("Micros", &inputs, default_in, true),
        ("Sorties", &outputs, default_out, false),
    ] {
        println!("{title} :");
        for (i, e) in entries.iter().enumerate() {
            let cfg = if input {
                e.device.default_input_config()
            } else {
                e.device.default_output_config()
            };
            let fmt = match cfg {
                Ok(c) => format!("{} Hz, {} canaux, {:?}", c.sample_rate(), c.channels(), c.sample_format()),
                Err(err) => format!("format illisible ({err})"),
            };
            let star = if Some(i) == default { " [défaut]" } else { "" };
            println!("  - {}{} : {}", e.name, star, fmt);
        }
    }
}

/// Diagnostic : ouvre « CABLE Input » en silence en mode faible latence (`--probe-lowlat`).
pub fn probe_lowlat() {
    let host = cpal::default_host();
    let (outputs, _) = list_outputs(&host);
    let names: Vec<String> = outputs.iter().map(|e| e.name.clone()).collect();
    let Some(i) = cable_input_index(&names) else {
        println!("Aucune sortie « CABLE Input ».");
        return;
    };
    match outputs[i].device.id() {
        Ok(id) => crate::lowlat::probe(id.id(), 3),
        Err(e) => println!("Identifiant illisible : {e}"),
    }
}

/// Index a preselectionner : le peripherique par defaut de Windows, sinon le premier.
pub fn default_index(names: &[String], default_name: Option<&str>) -> Option<usize> {
    if names.is_empty() {
        return None;
    }
    default_name
        .and_then(|d| names.iter().position(|n| n == d))
        .or(Some(0))
}

/// Index du peripherique "CABLE Input" (entree du micro virtuel VB-CABLE), s'il existe.
pub fn cable_input_index(output_names: &[String]) -> Option<usize> {
    output_names
        .iter()
        .position(|n| n.to_lowercase().contains("cable input"))
}

/// Vrai pour les peripheriques du cable virtuel (jamais un vrai micro).
pub fn is_virtual_cable(name: &str) -> bool {
    name.to_lowercase().contains("cable")
}

/// Micro a preselectionner : celui enregistre s'il existe, sinon le defaut Windows, en evitant le
/// cable virtuel (qui boucle sur la sortie de ClearMic) tant qu'un vrai micro existe.
pub fn pick_input(names: &[String], saved: Option<&str>, default_name: Option<&str>) -> Option<usize> {
    let real = |i: &usize| !is_virtual_cable(&names[*i]);
    saved
        .and_then(|s| names.iter().position(|n| n == s))
        .filter(real)
        .or_else(|| default_name.and_then(|d| names.iter().position(|n| n == d)).filter(real))
        .or_else(|| (0..names.len()).find(real))
        .or_else(|| default_index(names, default_name))
}

/// Refuse les sorties qui creeraient un larsen ou doubleraient le micro virtuel pour le retour micro.
pub fn check_monitor_target(monitor: &str, main_output: &str) -> Result<(), String> {
    if monitor.to_lowercase().contains("cable") {
        return Err(format!(
            "Le retour micro ne peut pas utiliser « {monitor} » : choisissez votre casque ou vos haut-parleurs."
        ));
    }
    if monitor == main_output {
        return Err(format!(
            "Le retour micro ne peut pas utiliser la même sortie que le micro virtuel (« {monitor} »)."
        ));
    }
    Ok(())
}

/// Verifie que le format natif du peripherique est utilisable (48 kHz, f32).
pub fn validate_format(name: &str, rate: u32, format: SampleFormat) -> Result<(), String> {
    if rate != REQUIRED_RATE {
        return Err(format!(
            "Réglez ce périphérique sur 48000 Hz dans les paramètres son de Windows : « {name} » (actuellement {rate} Hz)."
        ));
    }
    if format != SampleFormat::F32 {
        return Err(format!(
            "Format audio non pris en charge pour « {name} » ({format:?}) : 32 bits flottants attendus."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_is_preselected() {
        let n = names(&["A", "B", "C"]);
        assert_eq!(default_index(&n, Some("B")), Some(1));
    }

    #[test]
    fn unknown_default_falls_back_to_first() {
        let n = names(&["A", "B"]);
        assert_eq!(default_index(&n, Some("Z")), Some(0));
        assert_eq!(default_index(&n, None), Some(0));
    }

    #[test]
    fn empty_list_has_no_selection() {
        assert_eq!(default_index(&[], Some("A")), None);
    }

    #[test]
    fn cable_input_is_detected() {
        let n = names(&["Haut-parleurs (Realtek)", "CABLE Input (VB-Audio Virtual Cable)"]);
        assert_eq!(cable_input_index(&n), Some(1));
    }

    #[test]
    fn monitor_refuses_virtual_cable_and_main_output() {
        assert!(check_monitor_target("CABLE Input (VB-Audio Virtual Cable)", "X").is_err());
        assert!(check_monitor_target("CABLE Output (VB-Audio Virtual Cable)", "X").is_err());
        assert!(check_monitor_target("Casque (USB)", "Casque (USB)").is_err());
        assert!(check_monitor_target("Casque (USB)", "CABLE Input (VB-Audio Virtual Cable)").is_ok());
    }

    #[test]
    fn input_pick_avoids_virtual_cable() {
        let n = names(&["CABLE Output (VB-Audio Virtual Cable)", "Micro (USB)"]);
        assert_eq!(pick_input(&n, None, Some("CABLE Output (VB-Audio Virtual Cable)")), Some(1));
        assert_eq!(pick_input(&n, Some("CABLE Output (VB-Audio Virtual Cable)"), None), Some(1));
        assert_eq!(pick_input(&n, Some("Micro (USB)"), None), Some(1));
        let only_cable = names(&["CABLE Output (VB-Audio Virtual Cable)"]);
        assert_eq!(pick_input(&only_cable, None, None), Some(0));
        assert_eq!(pick_input(&[], None, None), None);
    }

    #[test]
    fn missing_cable_is_reported() {
        let n = names(&["Haut-parleurs (Realtek)", "CABLE Output (VB-Audio Virtual Cable)"]);
        assert_eq!(cable_input_index(&n), None);
    }

    #[test]
    fn rejects_44100_with_device_name() {
        let e = validate_format("Mon micro", 44_100, SampleFormat::F32).unwrap_err();
        assert!(e.contains("48000 Hz"));
        assert!(e.contains("Mon micro"));
    }

    #[test]
    fn rejects_non_float_format() {
        assert!(validate_format("M", 48_000, SampleFormat::I16).is_err());
    }

    #[test]
    fn accepts_48k_float() {
        assert!(validate_format("M", 48_000, SampleFormat::F32).is_ok());
    }
}
