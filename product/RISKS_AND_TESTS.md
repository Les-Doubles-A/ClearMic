# ClearMic - Risques et tests

> Version 1 - 2026-10-20

## 1. Risques

| ID | Risque | Type | Probabilité | Impact | Prévention | Repli |
|---|---|---|---|---|---|---|
| R1 | Latence supérieure à 30 ms (période minimale non accessible via cpal) | Technique | Moyenne | Fort | Mesure en T2, décision D2 sur preuve | IAudioClient3 direct, puis mode exclusif |
| R2 | Qualité RNNoise inférieure à Krisp (H3) | Fonctionnel | Moyenne | Fort | Test A/B M3 en tranche 1 | Autre moteur libre ; accepter la qualité |
| R3 | Dérive d'horloge entre micro, câble et casque : craquements | Technique | Moyenne | Moyen | Correction de dérive et ring bufferisés, test simulé | Augmenter le tampon cible |
| R4 | VB-CABLE : conditions de licence ou disponibilité changent | Légal / Coût | Faible | Moyen | Relire les conditions avant usage prolongé **[À vérifier]** | Autre câble virtuel |
| R5 | Une application (Discord, jeu) applique sa propre suppression de bruit ou son contrôle automatique de gain sur "CABLE Output" | Plateforme | Moyenne | Moyen | Documenter dans M1 la désactivation de ces options | Aucun côté ClearMic |
| R6 | Larsen avec le retour micro | Fonctionnel | Moyenne | Moyen | Retour désactivé par défaut, refus des mauvais périphériques (AC-F6-2) | Utiliser un casque |
| R7 | Saturation avec le boost (+30 dB) | Fonctionnel | Moyenne | Moyen | Limiteur actif par défaut (AC-F3-2) | Réduire le boost |
| R8 | Périphériques non réglés sur 48 kHz | Plateforme | Moyenne | Faible | Message guidé (AC-F1-5) | Rééchantillonnage en V2 |
| R9 | Licences des dépendances incompatibles avec une diffusion future | Légal | Faible | Faible | Vérification en T3 puis avant diffusion | Remplacer la dépendance |
| R10 | Permission micro Windows refusée pour une application de bureau | Plateforme | Faible | Moyen | Message d'aide vers les paramètres de confidentialité **[À vérifier]** | Activer la permission à la main |

## 2. Hypothèses non vérifiées

| ID | Hypothèse | Comment la vérifier | Quand | Ce qu'on fait si elle est fausse |
|---|---|---|---|---|
| H1 | Application Windows et non extension | Demander à l'utilisateur | Avant T1 | Refaire le plan |
| H2 | VB-CABLE comme micro virtuel | M1 | T2 | Autre câble virtuel |
| H3 | RNNoise suffisant | M3 | T3 | Autre moteur |
| H4 | 30 ms atteignables | M2 | T2 | Viser 40 ms, IAudioClient3 direct ou exclusif |
| H5 | Périphériques en 48 kHz | AC-F1-5 | T2 | Rééchantillonneur temps réel |
| H6 | Un micro, sortie 2 canaux | M1 | T2 | Multi-micros en V2 |
| H7 | Pads de 30 s maximum en mémoire | AC-F7-1 et AC-F7-4 | T9 | Lecture en flux |
| H8 | Interface en français | Revue | T10 | Internationalisation en V2 |

## 3. Stratégie de test

| Niveau | Outil | Ce qui est couvert | Quand l'exécuter |
|---|---|---|---|
| Unitaire | `cargo test` (crate DSP, sinus et bruit de référence) | Effets, mixeur, estimation de latence, configuration, décodage | À chaque modification |
| Intégration | `cargo test` avec flux et périphériques simulés | Dérive, listes de périphériques, file de commandes | À chaque modification |
| Bout en bout | Aucun automatisable (matériel audio) | Sans objet | Sans objet |
| Manuel | Scénarios M1 à M7 | Audio réel, latence, qualité | Avant chaque livraison |

**Commande unique** : `cargo test --workspace`

## 4. Couverture des critères d'acceptation

| Critère | Vérification | Test ou scénario | Tâche |
|---|---|---|---|
| AC-F1-1 | Automatique + manuelle | test liste simulée ; M1 | T2 |
| AC-F1-2 | Manuelle | M1 | T2 |
| AC-F1-3 | Manuelle | M2 | T2, T10 |
| AC-F1-4 | Manuelle | M4 | T2, T10 |
| AC-F1-5 | Automatique + manuelle | test format simulé ; M1 | T2 |
| AC-F1-6 | Manuelle | M1 (étape de débranchement) | T2 |
| AC-F2-1 | Automatique | test bruit blanc | T3 |
| AC-F2-2 | Automatique | test intensité | T3 |
| AC-F2-3 | Manuelle | M3 | T3 |
| AC-F2-4 | Automatique | test bascule | T3 |
| AC-F2-5 | Automatique + manuelle | test latence ; M2 | T3 |
| AC-F3-1 | Automatique | test gain | T5 |
| AC-F3-2 | Automatique | test boost et limiteur | T5 |
| AC-F3-3 | Automatique | test écrêtage | T5 |
| AC-F3-4 | Automatique | test mono | T5 |
| AC-F3-5 | Automatique | test balance | T5 |
| AC-F3-6 | Automatique | test largeur | T5 |
| AC-F4-1 | Automatique | test bypass EQ | T6 |
| AC-F4-2 | Automatique | test cloche | T6 |
| AC-F4-3 | Automatique | test passe-haut | T6 |
| AC-F4-4 | Automatique | test bornes | T6 |
| AC-F4-5 | Automatique | test lissage | T6 |
| AC-F5-1 | Automatique | test régime établi | T7 |
| AC-F5-2 | Automatique | test sous le seuil | T7 |
| AC-F5-3 | Automatique | test attaque | T7 |
| AC-F5-4 | Automatique | test makeup | T7 |
| AC-F5-5 | Manuelle | M6 | T7 |
| AC-F6-1 | Manuelle | M6 | T8 |
| AC-F6-2 | Automatique + manuelle | test exclusion ; M6 | T8 |
| AC-F6-3 | Manuelle | M6 | T8 |
| AC-F6-4 | Manuelle | M6 | T8 |
| AC-F6-5 | Manuelle | M6 | T8 |
| AC-F7-1 | Automatique + manuelle | test décodage ; M7 | T9 |
| AC-F7-2 | Automatique + manuelle | test file de commandes ; M7 | T9 |
| AC-F7-3 | Automatique | test volumes et relance | T9 |
| AC-F7-4 | Automatique | test fichiers invalides | T9 |
| AC-F7-5 | Automatique | test chemin absent | T9 |
| AC-F7-6 | Manuelle | M7 | T9 |
| AC-F8-1 | Automatique + manuelle | test aller-retour ; M1 | T4 |
| AC-F8-2 | Automatique | test fichier corrompu | T4 |
| AC-F8-3 | Automatique | test écriture interrompue | T4 |
| AC-F8-4 | Automatique | test périphérique absent | T4 |

## 5. Scénarios de test manuels

### M1 - Installation et premier démarrage

- **Préparation** : VB-CABLE installé ; micro et "CABLE Input" réglés sur 48000 Hz dans Windows ; dans Discord ou OBS, désactiver suppression de bruit et contrôle automatique du gain (R5).
- **Étapes** : lancer ClearMic ; vérifier les listes ; Démarrer ; parler ; choisir "CABLE Output" comme micro dans une application d'enregistrement ; changer un micro en 44100 Hz et vérifier le message ; débrancher puis rebrancher le micro ; relancer ClearMic.
- **Résultat attendu** : voix captée sur "CABLE Output", message de format nommant le périphérique, arrêt sous 2 s au débranchement, reprise sans relancer, réglages restaurés.
- **Critères couverts** : AC-F1-1, AC-F1-2, AC-F1-5, AC-F1-6, AC-F8-1

### M2 - Mesure de la latence

- **Préparation** : enregistreur multipiste (OBS ou équivalent **[À vérifier]**) avec deux pistes : le micro réel et "CABLE Output". Tous effets désactivés sauf débruitage, puis répétition avec tous effets.
- **Étapes** : claquer des mains 10 fois ; mesurer le décalage entre les deux pistes (le décalage propre à l'enregistreur est estimé avec un second enregistrement du micro réel sur deux pistes) ; comparer à la latence affichée.
- **Résultat attendu** : décalage moyen de 30 ms ou moins ; écart entre estimation et mesure consigné.
- **Critères couverts** : AC-F1-3, AC-F2-5

### M3 - Test A/B du débruitage

- **Préparation** : ventilateur ou bruit de clavier près du micro ; Krisp installé pour la comparaison.
- **Étapes** : enregistrer 30 s de parole avec bruit sur : (a) sans traitement, (b) RNNoise à 100 %, (c) Krisp. Écouter à l'aveugle.
- **Résultat attendu** : (b) intelligible, sans artefact gênant, jugé acceptable face à (c).
- **Critères couverts** : AC-F2-3

### M4 - Stabilité

- **Étapes** : tous effets activés, retour micro et soundboard en marche, enregistrer "CABLE Output" 10 minutes pendant une partie ou une tâche CPU habituelle.
- **Résultat attendu** : underruns à 0, pas de craquement audible.
- **Critères couverts** : AC-F1-4

### M5 - Pas de réseau

- **Étapes** : surveiller les connexions de ClearMic avec le moniteur de ressources de Windows pendant une session complète.
- **Résultat attendu** : aucune connexion réseau.
- **Critères couverts** : contrainte "gratuit, local" (critères transverses)

### M6 - Retour micro et compresseur

- **Étapes** : activer le retour sur un casque, vérifier le refus de "CABLE Input", activer la case soundboard, jouer un pad, débrancher le casque, parler fort puis doucement avec le compresseur actif.
- **Résultat attendu** : voix entendue dans le casque ; latence du retour affichée à 30 ms ou moins ; la sortie vers le câble continue après le débranchement ; l'indicateur de réduction de gain bouge et revient à 0 dB.
- **Critères couverts** : AC-F5-5, AC-F6-1, AC-F6-2, AC-F6-3, AC-F6-4, AC-F6-5

### M7 - Soundboard

- **Étapes** : glisser un WAV, un MP3, un OGG, un FLAC et un fichier texte ; ajouter 16 pads ; en déclencher 16 en moins d'une seconde pendant 1 minute ; renommer un fichier et relancer.
- **Résultat attendu** : pads créés pour les 4 formats audio, message pour le fichier texte, aucun underrun, pad grisé "Fichier introuvable" au relancement.
- **Critères couverts** : AC-F7-1, AC-F7-2, AC-F7-6

## 6. Validation du MVP

Le MVP est validé quand :

- [ ] tous les critères d'acceptation MVP sont vérifiés (tableau de la section 4) ;
- [ ] `cargo test --workspace` passe sans échec ;
- [ ] M1 à M7 ont été exécutés sur le poste de l'utilisateur ;
- [ ] latence de 30 ms ou moins (40 ms au plus avec dérogation consignée) et 0 underrun sur 10 minutes ;
- [ ] M3 est favorable (H3).

## 7. Après le lancement

Observer pendant 2 semaines : retour à Krisp ou non, craquements, bruit résiduel, réglages jamais utilisés. Ces signaux décident de F9 à F12.
