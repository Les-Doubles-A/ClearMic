# ClearMic - Architecture technique

> Version 1 - 2026-10-20 - Nouveau projet, aucune stack existante à respecter (dossier vide).

## 1. Choix techniques

| Domaine | Choix | Justification | Alternative écartée |
|---|---|---|---|
| Plateforme | Application de bureau Windows 10/11, un seul exécutable portable (H1, H9) | Micro virtuel, retour micro et soundboard exigent l'accès aux périphériques système | Extension navigateur : audio limité aux onglets |
| Langage | Rust **[Reco]** | Pas de ramasse-miettes (latence prévisible), binaire unique, tests unitaires simples pour le traitement du signal | Python (latence), C++ avec framework audio complet (licence à vérifier, build plus lourd), Electron/Web Audio (latence plus élevée et dépendance lourde) **[Reco]** |
| E/S audio | Bibliothèque cpal sur WASAPI en mode partagé | Pur Rust, flux d'entrée et de sortie indépendants. Les périodes courtes en mode partagé existent via IAudioClient3 depuis Windows 10 (Microsoft Learn) ; **[Fait, lu dans le code de cpal 0.18.2 le 2026-10-20]** cpal n'utilise pas IAudioClient3 et ignore la taille de tampon demandée en mode partagé (période = `GetDevicePeriod()`) ; le repli prévu est l'appel direct de IAudioClient3 via la crate `windows` si M2 dépasse 40 ms (décision D2, voir T2) | WASAPI exclusif : latence minimale mais bloque le micro pour les autres applications |
| Micro virtuel | VB-CABLE (installé par l'utilisateur) : ClearMic écrit dans "CABLE Input", les applications lisent "CABLE Output" (H2) | Écrire un pilote audio signé est hors de portée d'un projet personnel | Pilote maison ; fonction pilote de Voicemeeter ou autres **[À vérifier]** |
| Débruitage | RNNoise via le portage Rust `nnnoiseless` (BSD-3-Clause selon un listing tiers) **[À vérifier]** dans LICENSE | Libre, temps réel, trames de 480 échantillons à 48 kHz (10 ms) | Krisp (SDK payant ou fermé), moteur ML maison |
| DSP | Modules maison sans dépendance : biquad (EQ, passe-haut), compresseur, limiteur, gain, canaux | Petits, testables avec des sinus | Plugins VST : hors périmètre |
| Interface | egui via eframe **[À vérifier]** | Une fenêtre, un seul binaire, glisser-déposer de fichiers pris en charge | Tauri/web : second runtime, plus lourd |
| Décodage fichiers | symphonia (WAV, MP3, OGG, FLAC) et rubato pour passer à 48 kHz **[À vérifier]** versions, licences et formats | Pur Rust, décodage hors thread audio | Bibliothèques C via FFI |
| Mémoire entre threads | File SPSC sans verrou (crate `ringbuf` **[À vérifier]**) et atomiques pour les réglages | Aucun verrou ni allocation dans le rappel audio | Mutex : risque d'inversion de priorité |
| Persistance | `config.json` dans `%APPDATA%\ClearMic\` (serde) | Données simples, lisibles, éditables | Base embarquée : inutile |
| Tests | `cargo test` (DSP pur) + scénarios manuels | Le DSP est séparé de l'E/S, donc testable sans carte son | - |

## 2. Composants

```
Micro -> [cpal capture] -> ring A -> thread DSP/rappel de sortie
   gain -> RNNoise -> HPF+EQ -> compresseur -> canaux -> boost -> limiteur
   -> + soundboard (lecteur de pads) -> sortie "CABLE Input"
                                     \-> ring B -> [cpal sortie retour] -> casque
```

- **Crate `clearmic-dsp` (bibliothèque pure)** : trait `Effect`, tous les effets, mixeur, lecteur de pads. Aucune dépendance audio système.
- **Crate `clearmic-app` (binaire)** : E/S cpal, gestion des périphériques, interface egui, configuration.
- **Threads** : rappel de capture (copie vers ring A), rappel de sortie principal (traitement complet par trames de 480 échantillons), rappel de sortie retour (lecture du ring B), thread d'interface, thread de décodage des fichiers. Seuls les rappels audio sont temps réel : pas d'allocation, de verrou, de fichier ni d'affichage.
- **Dérive d'horloge** : micro et sortie ont des horloges distinctes ; le ring A garde un remplissage cible (par exemple 1 période) et corrige par suppression ou duplication d'un échantillon lorsque le remplissage dérive. Idem pour le ring B.
- **Priorité des threads** : demander la classe MMCSS "Pro Audio" aux threads audio via la crate `windows` **[À vérifier]** que cpal ne le fait pas déjà.
- **Budget de latence (cible 30 ms, H4)** : période de capture + trame RNNoise de 10 ms + période de sortie + tampon de VB-CABLE. Les valeurs réelles sont mesurées en T2 et en M2 ; l'indicateur affiché est la somme des périodes lues sur les flux plus 10 ms quand le débruitage est actif.
- **Format interne** : f32, 48000 Hz ; sortie 2 canaux ; micro mono dupliqué, micro stéréo conservé (H5, H6).

## 3. Local vs serveur

| Fonction | Où | Service externe | Clé secrète | Repli |
|---|---|---|---|---|
| Tout le traitement audio | Local | Non | Non | Sans objet |
| Micro virtuel | Local, pilote VB-CABLE tiers | Pilote installé séparément | Non | Message "VB-CABLE absent" (AC-F1-1) ; sortie vers n'importe quel autre périphérique possible |
| Débruitage | Local (RNNoise) | Non | Non | Option désactivable |
| Soundboard, configuration | Local, fichiers | Non | Non | Sans objet |

Aucun serveur, aucune API payante, aucun accès réseau : la contrainte "gratuit" est respectée ligne par ligne. Coût récurrent : 0. Coût ponctuel possible : contribution à VB-Audio selon ses conditions **[À vérifier]**.

## 4. Données

Fichier `config.json` : versions du schéma, noms des périphériques (pas leurs identifiants instables), paramètres des effets, liste des pads (chemin, nom, volume). Écriture atomique (AC-F8-3). Export manuel = copie du fichier. Les sons restent à leur emplacement d'origine (pas de copie).

## 5. Sécurité et erreurs

- Aucune clé, aucun secret ; pas de `.env` nécessaire (sans objet).
- Fichiers audio : décodés par symphonia dans le thread de décodage, taille et durée plafonnées (30 s) avant allocation complète ; échec = message, pas de plantage.
- Aucune requête réseau ; pas de télémétrie.
- Erreurs : message lisible dans l'interface, détail dans `%APPDATA%\ClearMic\clearmic.log` (rotation simple).
- Licences : vérifier celles de cpal, nnnoiseless, symphonia, rubato, egui avant de distribuer ; usage perso donc faible enjeu, mais obligatoire si le projet est ouvert plus tard.

## 6. Déploiement et maintenance

`cargo build --release`, exécutable portable, pas d'installeur ni de signature (H9). Windows SmartScreen peut avertir **[À vérifier]**. Mise à jour = remplacer l'exécutable.

## 7. Décisions

Les décisions D1 à D4 sont dans IMPLEMENTATION_PLAN.md.
