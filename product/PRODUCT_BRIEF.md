# ClearMic - Brief produit

> Version 1 - 2026-10-20 - Statut : brouillon de planification

Légende : **[Fait]** vérifié (source ou exprimé par l'utilisateur) · **[H#]** hypothèse · **[Reco]** recommandation · **[À vérifier]**.

## 1. Concept

ClearMic est une application Windows qui capte le micro, nettoie et mixe la voix en temps réel avec la latence la plus basse possible, puis l'envoie vers un micro virtuel que Discord, OBS, Teams ou un jeu utilisent comme micro. Elle remplace l'usage de Krisp en version gratuite, avec plus de réglages : suppression de bruit, gain, boost en dB, mono/stéréo, égaliseur, compresseur, retour micro (monitoring) et soundboard de fichiers locaux.

## 2. Problème et utilisateur

- **Utilisateur** : l'utilisateur seul, usage personnel **[Fait : "Usage perso d'abord", exprimé par l'utilisateur]**, qui parle en vocal (jeu, streaming, réunions) depuis un PC Windows.
- **Problème** : Krisp gratuit limite la suppression de bruit à un quota quotidien (60 min/jour selon des agrégateurs de prix tiers, non confirmé sur le site officiel **[À vérifier]**) et offre peu de réglages audio. Les autres réglages (EQ, compresseur, soundboard) obligent à empiler plusieurs logiciels.
- **Fréquence/coût** : à chaque session vocale ; coût = abonnement ou bruit/voix mal réglée.

## 3. Alternatives

| Alternative | Limite pour cette cible |
|---|---|
| Krisp gratuit | Quota quotidien **[À vérifier]**, réglages audio limités, pas de soundboard |
| Equalizer APO | **[Fait]** EQ système sous licence GPLv2, agit via l'infrastructure d'effets Windows et ne traite pas les applications en WASAPI exclusif ou ASIO (https://sourceforge.net/projects/equalizerapo/, consulté le 2026-10-20). Pas de débruitage ni de soundboard |
| Mixeurs audio virtuels, filtres de logiciels de streaming (catégories) | Plusieurs outils à assembler, configuration complexe, latence à régler **[À vérifier]** |
| Ne rien faire | Bruit de fond transmis tel quel |

## 4. Proposition de valeur

Un seul outil gratuit, sans quota, avec tous les réglages voix demandés et une latence mesurée et affichée. **Honnêteté** : la qualité de RNNoise (moteur libre prévu) peut être inférieure à celle de Krisp ; c'est l'hypothèse critique H3.

## 5. Hypothèses

| ID | Hypothèse retenue | Si fausse |
|---|---|---|
| H1 | Plateforme : application de bureau Windows 10/11 (la question n'a pas reçu de réponse ; le nom de dossier évoque une extension, mais le micro virtuel, le retour micro et le soundboard vers d'autres applications sont impossibles dans un navigateur) | Une extension navigateur ne traiterait que l'audio des sites web : plan à refaire |
| H2 | Le micro virtuel est fourni par VB-CABLE installé séparément par l'utilisateur ; pas de pilote audio maison | Écrire un pilote signé : hors périmètre, coût élevé |
| H3 | Le débruitage RNNoise est jugé suffisant par l'utilisateur face à Krisp (test A/B à l'écoute) | Évaluer un autre moteur libre ; la qualité devient un risque produit |
| H4 | Une latence micro vers micro virtuel de 30 ms ou moins est atteignable en WASAPI mode partagé | Viser 40 ms, ou passer en mode exclusif / IAudioClient3 direct |
| H5 | Micro et sorties sont réglés sur 48000 Hz dans Windows ; sinon message d'erreur au MVP (rééchantillonnage temps réel en V2) | Ajouter un rééchantillonneur temps réel dès le MVP |
| H6 | Un seul micro à la fois ; sortie vers le micro virtuel en 2 canaux, 32 bits flottants en interne | Gestion multi-micros en V2 |
| H7 | Soundboard : fichiers WAV, MP3, OGG, FLAC de 30 s maximum, décodés en mémoire, 16 pads maximum | Lecture en flux depuis le disque (plus complexe) |
| H8 | Interface en français uniquement | Internationalisation en V2 |
| H9 | Distribution : exécutable portable non signé, usage personnel | Installeur et signature de code à prévoir si diffusion ouverte |

## 6. Objectifs et indicateurs (usage personnel, 2 semaines)

| Indicateur | Seuil |
|---|---|
| Latence micro vers micro virtuel mesurée (M2) | 30 ms ou moins (H4) |
| Session de 10 minutes sans coupure ni craquement (compteur d'underruns) | 0 |
| Remplace Krisp au quotidien pendant 2 semaines | Oui / non, déclaré par l'utilisateur |
| Test A/B débruitage RNNoise vs Krisp | RNNoise jugé acceptable (H3) |

## 7. Périmètre

- **MVP** : F1 à F8 (voir FEATURES_AND_ACCEPTANCE.md). Huit fonctionnalités, au-dessus de la fourchette habituelle : chacune est explicitement demandée et petite (un module de traitement du signal). La tranche 1 se limite à F1 et F2.
- **Version 2** : F9 à F12.
- **Hors périmètre** : voir FEATURES_AND_ACCEPTANCE.md section 1.

## 8. Test d'intérêt avant d'investir

Avant de coder l'interface complète : T2 (micro vers VB-CABLE en direct) puis T3 (RNNoise). Si, après T3, la qualité est jugée inacceptable face à Krisp (H3) ou la latence dépasse 40 ms (H4), arrêter ou réviser le moteur avant de construire le reste.

## 9. Questions ouvertes

- Confirmer H1 (application Windows et non extension).
- Confirmer les conditions d'usage de VB-CABLE : donationware, usage personnel avec contribution au choix de l'utilisateur, par appareil ; défaut affiché 15 USD sur la boutique **[À vérifier]** (https://shop.vb-audio.com/en/content/3-Terms-of-use et https://vb-audio.com/Services/licensing.htm, consultés le 2026-10-20). Le téléchargement de test gratuit existe pour VB-CABLE de base.
- Quota exact du plan gratuit Krisp : vérifier sur le site officiel.

## 10. Sources consultées (2026-10-20)

- RNNoise : licence BSD-3-Clause, plus CC0-1.0 sur certaines distributions (portée non précisée) ; dépôt amont https://gitlab.xiph.org/xiph/rnnoise.
- nnnoiseless (portage Rust de RNNoise) : licence BSD-3-Clause selon un listing tiers ; **[À vérifier]** dans le fichier LICENSE du dépôt.
- IAudioClient3 : périodes courtes en mode partagé depuis Windows 10, https://learn.microsoft.com/en-us/windows/desktop/api/audioclient/nn-audioclient-iaudioclient3 et https://learn.microsoft.com/en-ie/Windows/Win32/coreaudio/exclusive-mode-streams.
