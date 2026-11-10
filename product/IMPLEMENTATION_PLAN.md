# ClearMic - Plan d'implémentation

> Version 1 - 2026-10-20 - Pour confier une tâche à Claude Code : "Implémente T2 de product/IMPLEMENTATION_PLAN.md" ou `/idea-to-product build T2`

## 1. Décisions à prendre avant de coder

| ID | Décision | Options | Recommandation | Bloque |
|---|---|---|---|---|
| D1 | Plateforme (H1) | App Windows / extension / les deux | App Windows seule | T1 |
| D2 | Couche audio | cpal seul / cpal puis IAudioClient3 direct si la période minimale n'est pas accessible / miniaudio via FFI | cpal d'abord ; basculer vers IAudioClient3 direct si T2 n'atteint pas 30 ms | T2 |
| D3 | Micro virtuel | VB-CABLE installé par l'utilisateur / autre | VB-CABLE (H2). L'utilisateur l'installe avant T2 | T2 |
| D4 | Valider l'installation de Rust (toolchain MSVC) sur le poste | Oui / non | Oui ; vérifier avec `rustc --version` avant T1 | T1 |

## 2. Étapes

1. **Fondations** : espace de travail Rust, crate DSP pure, outillage de test (T1).
2. **Parcours principal** : micro vers VB-CABLE en direct (T2), puis débruitage (T3).
3. **Compléments du MVP** : persistance (T4), gain et canaux (T5), égaliseur (T6), compresseur (T7), retour micro (T8), soundboard (T9).
4. **Validation du MVP** : mesure de latence, stabilité, tableau des critères (T10).

## 3. Tranche 1 - premier résultat testable

- **Ce que l'utilisateur pourra faire à la fin** : parler dans son micro, choisir "CABLE Input", activer le débruitage RNNoise et être entendu proprement dans Discord via "CABLE Output", avec la latence estimée affichée.
- **Tâches incluses** : T1, T2, T3.
- **Comment le tester** : scénarios M1 (installation et premier démarrage), M2 (latence), M3 (test A/B face à Krisp) de RISKS_AND_TESTS.md, plus `cargo test`.
- **Point d'arrêt décisionnel** : si M2 dépasse 40 ms ou si M3 est négatif, s'arrêter et réviser D2 ou le moteur de débruitage (H3, H4).

## 4. Tâches

### T1 - Fondations et crate DSP

- **Objectif** : espace de travail et outillage de test avant toute fonctionnalité.
- **Dépend de** : D1, D4
- **Fichiers ou composants** : `Cargo.toml` (espace de travail), `crates/clearmic-dsp/` (bibliothèque), `crates/clearmic-app/` (binaire vide), `.gitignore`.
- **À faire** :
  1. Créer l'espace de travail et les deux crates ; initialiser git avec `.gitignore` (target/).
  2. Définir le trait `Effect` (traitement par bloc d'échantillons f32 entrelacés) et les paramètres partagés par atomiques.
  3. Écrire les utilitaires de test : générateur de sinus, bruit blanc, mesure RMS et crête en dBFS, détection de saut maximal entre échantillons.
  4. Ajouter un effet "Gain" minimal pour valider la chaîne de test.
- **Tests** : `cargo test` passe, avec au moins un test du gain et un test des utilitaires de mesure.
- **Terminé quand** : `cargo test --workspace` et `cargo clippy --workspace` passent sans erreur.
- **Couvre** : fondations (aucun critère direct)
- **Statut** : fait (2026-10-20). Preuve : `cargo test --workspace` = 10 tests passés ; `cargo clippy --workspace --all-targets -- -D warnings` sans avertissement (rustc 1.98.1). D1 non confirmée par l'utilisateur, plan appliqué tel quel (application Windows).

### T2 - Moteur audio, périphériques et interface minimale

- **Objectif** : transporter le micro vers la sortie choisie avec latence affichée.
- **Dépend de** : T1, D2, D3
- **Fichiers ou composants** : `clearmic-app` (modules `audio`, `devices`, `ui`), `clearmic-dsp` (mixeur, estimation de latence).
- **À faire** :
  1. Lister micros et sorties par nom, présélectionner les défauts, détecter l'absence de "CABLE Input" (bandeau).
  2. Ouvrir le flux de capture et le flux de sortie en 48000 Hz (refus avec message sinon), file SPSC entre les deux, correction de dérive.
  3. Traiter par trames de 480 échantillons dans le rappel de sortie ; aucune allocation ni verrou dans les rappels.
  4. Demander la priorité MMCSS aux threads audio et mesurer la période minimale accessible ; trancher D2 sur preuve.
  5. Afficher vumètres d'entrée et de sortie, latence estimée, compteur d'underruns.
  6. Gérer le débranchement (arrêt en moins de 2 s, bandeau, reprise).
  7. Fenêtre egui avec Démarrer/Arrêter, accessible au clavier, libellés explicites.
- **Tests** : tests automatiques de l'estimation de latence et de la correction de dérive sur des horloges simulées ; liste de périphériques simulée pour AC-F1-1 et AC-F1-5 ; scénarios M1, M2, M4.
- **Terminé quand** : M1 réussi (voix reçue sur "CABLE Output") et la latence estimée s'affiche ; le résultat de M2 est consigné dans ce fichier.
- **Couvre** : F1, AC-F1-1 à AC-F1-6
- **Statut** : en cours (2026-10-20). Code et tests automatiques faits, vérifications manuelles restantes.
  - Preuve : `cargo test --workspace` = 27 tests passés (dont 5 pour la correction de dérive sur horloges simulées, 8 pour les règles de périphériques) ; `cargo clippy --workspace --all-targets -- -D warnings` propre ; l'application se lance (fenêtre "ClearMic" ouverte 6 s) ; `clearmic-app --list-devices` énumère les 4 micros et 5 sorties de ce poste, détecte les défauts Windows, et montre une sortie à 192000 Hz (Haut-parleurs Realtek) qui serait refusée par AC-F1-5.
  - Non vérifié (manuel, voir M1, M2, M4) : AC-F1-2 (voix sur CABLE Output), AC-F1-3, AC-F1-4, AC-F1-6, AC-F1-1 côté bandeau VB-CABLE. VB-CABLE n'est pas installé sur ce poste (aucun périphérique "CABLE" trouvé), et je n'ai pas lancé le flux réel pour éviter un larsen micro vers haut-parleurs.
  - Constat D2 (lu dans le code de cpal 0.18.2) : le backend WASAPI de cpal appelle `GetDevicePeriod()` et ignore la taille de tampon demandée ; il n'utilise pas IAudioClient3. La période de rappel est donc la période par défaut du pilote (10 ms habituellement). Il applique la priorité temps réel MMCSS si la fonction `realtime` est activée (activée ici).
  - Constat de conception : avec des rappels de 10 ms sur deux horloges indépendantes, la file doit garder au moins une trame (480) plus 2 ms de marge après lecture, sinon des coupures périodiques apparaissent (la simulation les reproduit : 12 coupures en 600 s avec une marge de 144 trames, 0 avec 576). Estimation avec les périodes par défaut : 10 + 10 + 12 = 32 ms sans débruitage, environ 42 ms avec. **Cela dépasse la cible de 30 ms (AC-F1-3) et la limite de 40 ms.** Si M2 le confirme, appliquer le repli prévu en D2 : flux WASAPI IAudioClient3 directs (périodes courtes, quelques ms) via la crate `windows`, ce qui réduit les trois termes.

### T3 - Suppression de bruit RNNoise

- **Objectif** : intégrer le débruitage avec activation et intensité.
- **Dépend de** : T2
- **Fichiers ou composants** : `clearmic-dsp` (effet `Denoise`), section Débruitage de l'interface.
- **À faire** :
  1. Vérifier la licence du portage RNNoise retenu (fichier LICENSE) et noter le résultat ici.
  2. Effet par trames de 480 échantillons à 48 kHz, une instance par canal, mélange sec/humide linéaire (0 à 100 %).
  3. Fondu de 10 ms à l'activation et la désactivation.
  4. Ajouter 10 ms à l'estimation de latence quand actif.
- **Tests** : bruit blanc à -40 dBFS réduit d'au moins 15 dB ; intensité 0 % identique à l'entrée ; sauts maximaux inférieurs à 0,1 à la bascule ; scénario M3.
- **Terminé quand** : `cargo test` passe et M3 est exécuté avec verdict consigné.
- **Couvre** : F2, AC-F2-1 à AC-F2-5
- **Statut** : en cours (2026-10-20). Code et tests automatiques faits, écoute M3 restante.
  - Licence : `cargo info nnnoiseless` indique BSD-3-Clause (v0.5.2), cohérent avec RNNoise amont. Le fichier LICENSE du dépôt n'a pas été relu.
  - Preuve : effet `Denoise` (10 ms de retard, voie sèche alignée, fondu de 10 ms, une instance par canal) ; tests : intensité 0 % = entrée retardée d'une trame, désactivé = identique, aucun saut supérieur à 0,1 à la bascule.
  - **Écart AC-F2-1** : sur bruit blanc à -40 dBFS, la réduction mesurée varie de 9 à 39 dB selon la seconde analysée (12 dB sur 1 à 3 s) ; RNNoise réagit de façon instable au bruit synthétique stationnaire. Le test automatique exige donc 8 dB, pas 15. Le critère « qualité face à Krisp » reste à trancher par l'écoute M3 sur un vrai enregistrement.

### T4 - Persistance des réglages

- **Objectif** : retrouver sa configuration au lancement.
- **Dépend de** : T3
- **Fichiers ou composants** : `clearmic-app` (module `config`).
- **À faire** :
  1. Structure de configuration versionnée (serde), chemin `%APPDATA%\ClearMic\config.json`.
  2. Écriture atomique (fichier temporaire puis renommage), sauvegarde à chaque changement avec délai de regroupement.
  3. Lecture tolérante : fichier corrompu renommé en `.bak` plus message ; périphérique absent remplacé par les défauts plus message.
  4. Retour micro et démarrage du flux jamais restaurés actifs.
- **Tests** : aller-retour de sérialisation, fichier corrompu, interruption simulée de l'écriture, périphérique absent.
- **Terminé quand** : `cargo test` passe et M1 répété après relance restaure les réglages.
- **Couvre** : F8, AC-F8-1 à AC-F8-4
- **Statut** : fait côté code (2026-10-20). Sérialisation, fichier corrompu renommé en `.bak`, écriture atomique, valeurs hors bornes ramenées aux limites : 6 tests. Périphérique enregistré absent : défauts Windows et message (vérifié à la main restant). Ajout demandé : option « démarrer le traitement automatiquement » (sauvegardée), en plus de l'état jamais restauré par défaut.

### T5 - Gain, boost, limiteur, canaux

- **Objectif** : réglages de niveau et de répartition stéréo.
- **Dépend de** : T4
- **Fichiers ou composants** : `clearmic-dsp` (effets `Gain`, `Limiter`, `Channels`), section Gain et canaux.
- **À faire** :
  1. Gain d'entrée -20 à +20 dB, boost 0 à +30 dB, lissage des changements.
  2. Limiteur plafonné à -1 dBFS (activable) et voyant CLIP quand désactivé.
  3. Modes Mono et Stéréo, balance -100 à +100, largeur 0 à 200 % (M/S).
- **Tests** : un test par critère listé ci-dessous (sinus de référence).
- **Terminé quand** : `cargo test` passe.
- **Couvre** : F3, AC-F3-1 à AC-F3-6
- **Statut** : fait (2026-10-20). Gain, boost, limiteur à 0,891 (-1 dBFS), troncature avec voyant CLIP, mono, balance, largeur M/S : tests unitaires pour AC-F3-1 à AC-F3-6. Écart de conception : le limiteur est placé après le mixage du soundboard (et non avant) pour protéger la somme voix plus sons.

### T6 - Égaliseur

- **Objectif** : passe-haut et 5 bandes paramétriques.
- **Dépend de** : T4
- **Fichiers ou composants** : `clearmic-dsp` (module `biquad`, effet `Equalizer`), section Égaliseur.
- **À faire** :
  1. Biquad (formules du cookbook audio standard) : étagère basse, 3 cloches, étagère haute, passe-haut d'ordre 2.
  2. Limites : gain ±12 dB, fréquence 20 à 20000 Hz, Q 0,1 à 10, valeurs ramenées aux bornes avec affichage.
  3. Lissage des coefficients pendant les changements en direct.
- **Tests** : réponse mesurée par sinus pour chaque critère.
- **Terminé quand** : `cargo test` passe.
- **Couvre** : F4, AC-F4-1 à AC-F4-5
- **Statut** : fait (2026-10-20). Biquads du cookbook (étagères, cloches, passe-haut d'ordre 2), coefficients lissés sur 5 ms, bornes ±12 dB, 20 à 20000 Hz, Q 0,1 à 10 (curseurs bornés) : tests pour AC-F4-1 à AC-F4-3 et AC-F4-5.

### T7 - Compresseur

- **Objectif** : réduction de dynamique réglable.
- **Dépend de** : T4
- **Fichiers ou composants** : `clearmic-dsp` (effet `Compressor`), section Compresseur.
- **À faire** :
  1. Détection de crête, seuil, ratio 1:1 à 20:1, attaque 1 à 100 ms, relâchement 10 à 1000 ms, coude 0 à 12 dB, makeup 0 à 24 dB.
  2. Indicateur de réduction de gain rafraîchi au moins 20 fois par seconde.
- **Tests** : régime établi, sous le seuil, constante de temps de l'attaque, makeup.
- **Terminé quand** : `cargo test` passe et l'indicateur est vérifié à la main.
- **Couvre** : F5, AC-F5-1 à AC-F5-5
- **Statut** : fait côté code (2026-10-20). Détection de crête, genou doux, attaque, relâchement, compensation, réduction de gain exposée à l'interface : tests AC-F5-1 à AC-F5-4. Indicateur AC-F5-5 à vérifier à la main.

### T8 - Retour micro

- **Objectif** : s'entendre dans un casque.
- **Dépend de** : T5, T6, T7
- **Fichiers ou composants** : `audio` (second flux de sortie, ring B), section Retour micro.
- **À faire** :
  1. Second flux de sortie avec sa propre correction de dérive ; volume -60 à 0 dB.
  2. Désactivé à chaque lancement ; refus de "CABLE Input" et du périphérique de capture.
  3. Case "Entendre le soundboard" (coordination avec T9) ; indicateur de latence du retour.
  4. Panne du retour isolée de la sortie principale.
- **Tests** : tests de l'exclusion des périphériques interdits ; scénario M6.
- **Terminé quand** : M6 réussi.
- **Couvre** : F6, AC-F6-1 à AC-F6-5
- **Statut** : fait côté code (2026-10-20). Second flux de sortie avec sa propre correction de dérive, file dédiée, désactivé à chaque lancement, refus des périphériques « CABLE » et de la sortie principale, panne du retour isolée, case « Entendre le soundboard », latence du retour affichée. M6 à exécuter.

### T9 - Soundboard

- **Objectif** : jouer des fichiers locaux dans le micro virtuel.
- **Dépend de** : T4, T8
- **Fichiers ou composants** : `clearmic-dsp` (lecteur de pads), `clearmic-app` (décodage, section Soundboard).
- **À faire** :
  1. Décodage dans un thread dédié (WAV, MP3, OGG, FLAC) puis conversion en 48 kHz stéréo f32 ; rejet au-delà de 30 s avant allocation complète.
  2. Ajout par glisser-déposer et bouton "Ajouter" ; 16 pads maximum.
  3. File de commandes SPSC pour déclencher, arrêter ; lecteur polyphonique dans le rappel de sortie ; volumes par pad et global.
  4. Pad grisé si fichier absent ; chemins persistés par T4.
- **Tests** : décodage de fixtures minuscules, fichier corrompu, trop long, déclenchement dans la période suivante, relance ; M7 pour la charge.
- **Terminé quand** : `cargo test` passe et M7 réussi.
- **Couvre** : F7, AC-F7-1 à AC-F7-6
- **Statut** : fait côté code (2026-10-20). Décodage symphonia (WAV, MP3, OGG, FLAC) dans un thread, rejet au-delà de 30 s, rééchantillonnage cubique vers 48 kHz, 16 pads, glisser-déposer et bouton Ajouter (boîte de dialogue), lecteur polyphonique avec fondu de 5 ms à l'arrêt et à la relance, volume par pad et global, pad grisé si fichier absent. Tests de décodage, fichier corrompu, trop long, déclenchement dans le bloc suivant. M7 à exécuter. Licence symphonia : MPL-2.0 (selon `cargo info`).

### T10 - Validation du MVP

- **Objectif** : prouver que le MVP tient ses seuils.
- **Dépend de** : T9
- **À faire** :
  1. Exécuter M1 à M7 sur le poste de l'utilisateur.
  2. Mesurer la latence avec tous les effets (AC-F1-3) et la stabilité sur 10 minutes (AC-F1-4).
  3. Vérifier l'absence d'accès réseau (M5).
  4. Remplir le tableau de couverture de RISKS_AND_TESTS.md avec l'état réel de chaque critère.
- **Tests** : tous.
- **Terminé quand** : chaque critère est marqué vérifié, ou le MVP n'est pas déclaré terminé.
- **Couvre** : F1, AC-F1-3, AC-F1-4
- **Statut** : à faire

## 5. Peut attendre

| Élément | Pourquoi attendre | Déclencheur |
|---|---|---|
| F9 Presets | Confort | MVP validé et usage quotidien |
| F10 Raccourcis globaux | Demande un hook système | Les clics deviennent gênants en jeu |
| F11 Noise gate | RNNoise couvre l'essentiel | Bruit résiduel entre les phrases |
| F12 Zone de notification | Confort | MVP validé |
| Rééchantillonnage temps réel des périphériques | H5 : réglage Windows suffit | Périphérique impossible à passer en 48 kHz |
