//! Lancement avec Windows : valeur `ClearMic` sous `HKCU\...\CurrentVersion\Run`.

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "ClearMic";
/// Argument passe au lancement automatique : fenetre reduite dans la barre des taches.
pub const MINIMIZED_FLAG: &str = "--minimized";

pub fn command_line(exe: &std::path::Path) -> String {
    format!("\"{}\" {MINIMIZED_FLAG}", exe.display())
}

pub fn is_enabled() -> bool {
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ) else {
        return false;
    };
    let Ok(current) = key.get_value::<String, _>(VALUE_NAME) else {
        return false;
    };
    // Une entree qui pointe vers un autre emplacement (dossier deplace) compte comme desactivee.
    std::env::current_exe().is_ok_and(|exe| current == command_line(&exe))
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey_with_flags(RUN_KEY, KEY_WRITE)
        .map_err(|e| format!("Registre inaccessible : {e}"))?;
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Chemin de l'application inconnu : {e}"))?;
        key.set_value(VALUE_NAME, &command_line(&exe))
            .map_err(|e| format!("Écriture dans le registre impossible : {e}"))
    } else {
        match key.delete_value(VALUE_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Suppression dans le registre impossible : {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quotes_the_path() {
        let s = command_line(std::path::Path::new(r"C:\Program Files\ClearMic\clearmic-app.exe"));
        assert_eq!(s, r#""C:\Program Files\ClearMic\clearmic-app.exe" --minimized"#);
    }
}
