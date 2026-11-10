# ClearMic - Fonctionnalités et critères d'acceptation

> Version 1 - 2026-10-20

## 1. Fonctionnalités et priorités

| ID | Priorité | Fonctionnalité | Justification (test de retrait) |
|---|---|---|---|
| F1 | MVP | Pipeline audio temps réel : micro vers micro virtuel, choix des périphériques, latence affichée | Sans lui, rien n'est audible |
| F2 | MVP | Suppression de bruit (RNNoise) avec activation et intensité | Remplace Krisp : valeur centrale |
| F3 | MVP | Gain d'entrée, boost en dB, limiteur, mono / stéréo (balance, largeur) | Demandé explicitement |
| F4 | MVP | Égaliseur (filtre passe-haut, 5 bandes) | Demandé explicitement |
| F5 | MVP | Compresseur | Demandé explicitement |
| F6 | MVP | Retour micro (monitoring) dans un casque | Demandé explicitement |
| F7 | MVP | Soundboard de fichiers locaux | Demandé explicitement |
| F8 | MVP | Persistance des réglages | Sans elle, tout se reconfigure à chaque lancement |
| F9 | V2 | Presets nommés (jeu, réunion, stream) | Confort, après validation du MVP |
| F10 | V2 | Raccourcis clavier globaux pour les pads | Demande un hook système ; clic suffit au MVP |
| F11 | V2 | Noise gate | RNNoise couvre le besoin principal |
| F12 | V2 | Icône de zone de notification et démarrage automatique | Confort |

**Hors périmètre** : moteur d'IA maison type Krisp (coût et expertise), pilote audio virtuel maison (signature de pilote), plugins VST, macOS et Linux, extension de navigateur (H1), comptes et synchronisation, enregistrement audio, effets de voix (pitch, réverbération).

**Ordre de la chaîne de traitement (fixe)** : gain d'entrée, débruitage (F2), filtre passe-haut et égaliseur (F4), compresseur (F5), canaux mono/stéréo (F3), boost (F3), limiteur (F3), mixage du soundboard (F7), sortie micro virtuel. Le retour micro (F6) copie la sortie finale.

## 2. Parcours principaux

**P1 - Première configuration** : l'utilisateur installe VB-CABLE, lance ClearMic, choisit son micro, choisit "CABLE Input" comme sortie, clique Démarrer, voit les vumètres bouger, puis choisit "CABLE Output" comme micro dans Discord.

**P2 - Régler sa voix** : avec le retour micro activé dans un casque, l'utilisateur active le débruitage, ajuste gain, égaliseur et compresseur, compare avec/sans, jusqu'à être satisfait. Les réglages sont conservés.

**P3 - Jouer un son** : l'utilisateur glisse un fichier sur la fenêtre, un pad apparaît, il clique dessus pendant qu'il parle ; l'interlocuteur entend la voix et le son.

## 3. Écran et états

Une seule fenêtre avec sections repliables : Périphériques et Démarrer/Arrêter, Débruitage, Gain et canaux, Égaliseur, Compresseur, Retour micro, Soundboard. Barre d'état : latence estimée, compteur d'underruns, vumètres entrée et sortie.

| État | Comportement |
|---|---|
| Vide | Aucun micro ou aucune sortie détecté : message "Aucun périphérique de capture/de rendu trouvé", Démarrer désactivé. Soundboard sans pad : "Glissez un fichier audio ici" |
| Chargement | Décodage d'un fichier du soundboard : pad en état "Chargement" |
| Erreur | Bandeau rouge avec cause et action (format, périphérique débranché, fichier invalide) |
| Succès | État "En marche" et vumètres actifs |

**Permissions** : accès micro de Windows (Confidentialité, Microphone, applications de bureau autorisées **[À vérifier]** pour une application non empaquetée) ; lecture des fichiers choisis par l'utilisateur. Aucun accès réseau (aucune requête sortante).

**Accessibilité** : tous les contrôles accessibles au clavier (Tab, flèches pour les curseurs), focus visible, libellés textuels sur chaque curseur avec unité, contraste suffisant, messages d'erreur affichés en texte (pas seulement en couleur). Le support des lecteurs d'écran dépend de la bibliothèque d'interface **[À vérifier]**.

## 4. Critères d'acceptation MVP

Mode de vérification : (A) test automatique, (M) manuel. Seuils sujets à H4 et H5.

### F1 - Pipeline audio temps réel

Objectif : transporter la voix du micro vers le micro virtuel avec une latence minimale, sans craquement.

**AC-F1-1** : Listes de périphériques (A+M)
Étant donné au moins un micro et une sortie Windows, au lancement les deux listes affichent les noms Windows des périphériques de capture et de rendu, avec les périphériques par défaut présélectionnés. Si aucune sortie dont le nom contient "CABLE Input" n'existe, un bandeau indique que VB-CABLE est absent avec l'adresse vb-audio.com/Cable ; les autres sorties restent sélectionnables.

**AC-F1-2** : Démarrage et vumètres (M)
Étant donné un micro, la sortie "CABLE Input" et les deux en 48000 Hz, quand l'utilisateur clique Démarrer, l'état passe à "En marche", le vumètre d'entrée suit sa voix, et un enregistreur réglé sur "CABLE Output" capte sa voix.

**AC-F1-3** : Latence (M, mesure en M2)
Avec toutes les options désactivées sauf le débruitage, la latence estimée affichée est de 30 ms ou moins, et la mesure acoustique/enregistrée du scénario M2 donne 30 ms ou moins (seuil H4 ; au-delà de 40 ms le MVP est refusé).

**AC-F1-4** : Stabilité (M)
Pendant 10 minutes en marche avec tous les effets activés, le compteur d'underruns affiché reste à 0 et aucun craquement n'est audible dans l'enregistrement de "CABLE Output".

**AC-F1-5** : Format non pris en charge (A+M)
Si le micro ou la sortie n'est pas en 48000 Hz, le démarrage est refusé avec le message "Réglez ce périphérique sur 48000 Hz dans les paramètres son de Windows" nommant le périphérique ; l'application ne plante pas.

**AC-F1-6** : Périphérique débranché (M)
Si le micro ou la sortie est débranché en marche, le flux s'arrête en moins de 2 secondes, un bandeau d'erreur nomme le périphérique, et Démarrer redevient utilisable après rebranchement sans relancer l'application.

### F2 - Suppression de bruit

Objectif : retirer les bruits de fond sans dégrader la voix.

**AC-F2-1** : Réduction du bruit stationnaire (A)
Étant donné un bruit blanc à -40 dBFS sans voix, activer le débruitage à 100 % abaisse le niveau RMS de sortie d'au moins 15 dB par rapport à désactivé.

**AC-F2-2** : Intensité (A)
À 0 %, la sortie est identique au signal d'entrée (aux délais d'alignement près) ; à 100 %, elle est la sortie pure du moteur ; entre les deux, le mélange est linéaire.

**AC-F2-3** : Voix préservée (M, M3)
Sur l'enregistrement de test de M3, l'utilisateur déclare la voix intelligible et sans artefact gênant à 100 %, et la qualité jugée acceptable face à Krisp (H3).

**AC-F2-4** : Bascule sans clic (A)
Activer ou désactiver le débruitage pendant un sinus de 440 Hz à -20 dBFS ne produit aucun saut entre échantillons consécutifs supérieur à 0,1 (fondu de 10 ms).

**AC-F2-5** : Latence ajoutée (A+M)
Activer le débruitage augmente la latence estimée de 10 ms au plus.

### F3 - Gain, boost, limiteur, canaux

Objectif : ajuster le niveau et la répartition stéréo de la voix.

**AC-F3-1** : Gain d'entrée (A)
Gain réglable de -20 à +20 dB par pas de 0,5 ; un sinus à -20 dBFS avec +6 dB donne -14 dBFS à 0,1 dB près (autres effets désactivés).

**AC-F3-2** : Boost et limiteur (A)
Boost réglable de 0 à +30 dB. Limiteur actif (défaut) : avec +20 dB sur un sinus à -6 dBFS, aucun échantillon de sortie ne dépasse 0,891 en valeur absolue (-1 dBFS).

**AC-F3-3** : Limiteur désactivé (A)
Limiteur désactivé : les valeurs sont tronquées à ±1,0 (jamais NaN ni infini) et le voyant CLIP s'allume tant que la troncature se produit.

**AC-F3-4** : Mode mono (A)
En mode Mono, pour toute entrée stéréo, L = R = (L + R) / 2 en sortie.

**AC-F3-5** : Balance (A)
En mode Stéréo, une source mono est dupliquée sur L et R ; avec balance à -100, R est silencieux ; à +100, L est silencieux ; à 0, L et R sont égaux pour une source mono.

**AC-F3-6** : Largeur stéréo (A)
Sur une source stéréo, largeur 0 % donne la sortie mono de AC-F3-4 ; 100 % laisse le signal inchangé ; 200 % double la composante latérale (M/S).

### F4 - Égaliseur

Objectif : façonner le timbre de la voix.

**AC-F4-1** : Bypass (A)
Égaliseur désactivé : la sortie est identique à l'entrée (aux délais d'alignement près).

**AC-F4-2** : Bande en cloche (A)
Bande 3 en cloche à 1 kHz, Q = 1, +6 dB : un sinus 1 kHz ressort +6 dB à 0,3 dB près ; un sinus 100 Hz varie de moins de 1 dB. Gain par bande limité à ±12 dB.

**AC-F4-3** : Filtre passe-haut (A)
Passe-haut à 80 Hz (ordre 2) : un sinus 40 Hz est atténué d'au moins 10 dB par rapport à un sinus 1 kHz.

**AC-F4-4** : Valeurs invalides (A)
Une fréquence hors 20 à 20000 Hz ou un Q hors 0,1 à 10 est ramené à la limite la plus proche, et le champ affiche la valeur appliquée.

**AC-F4-5** : Réglage en direct sans clic (A)
Déplacer un curseur de gain de bande pendant un sinus continu ne produit aucun saut entre échantillons consécutifs supérieur à 0,1 (coefficients lissés).

### F5 - Compresseur

Objectif : réduire les écarts de volume de la voix.

**AC-F5-1** : Taux de compression (A)
Seuil -20 dB, ratio 4:1, coude 0 dB, détection crête, makeup 0 : un sinus crête à -8 dBFS ressort à -17 dBFS à 0,5 dB près en régime établi.

**AC-F5-2** : Sous le seuil (A)
Un sinus à -40 dBFS avec seuil -20 dB ressort inchangé à 0,05 dB près.

**AC-F5-3** : Attaque (A)
Attaque 10 ms : après un saut de -40 à -8 dBFS, la réduction de gain atteint 63 % de sa valeur finale en 10 ms ± 2 ms.

**AC-F5-4** : Makeup (A)
Makeup réglable de 0 à +24 dB ; +6 dB augmente le niveau de sortie de 6 dB à 0,1 dB près.

**AC-F5-5** : Indicateur de réduction de gain (M)
Un indicateur affiche la réduction de gain en dB, rafraîchi au moins 20 fois par seconde, et revient à 0 dB sous le seuil.

### F6 - Retour micro

Objectif : s'entendre dans un casque avec le traitement appliqué.

**AC-F6-1** : Activation (M)
Avec le retour micro activé vers un casque, l'utilisateur entend sa voix traitée ; le volume est réglable de -60 à 0 dB.

**AC-F6-2** : Sécurité anti-larsen (A+M)
Le retour micro est désactivé à chaque lancement. Choisir "CABLE Input" ou le périphérique de capture comme sortie du retour est refusé avec un message.

**AC-F6-3** : Latence du retour (M)
Un indicateur distinct affiche la latence du retour ; elle est de 30 ms ou moins sur la machine de référence.

**AC-F6-4** : Soundboard dans le retour (M)
Une case "Entendre le soundboard" (cochée par défaut) décide si les pads sont audibles dans le casque.

**AC-F6-5** : Isolation des pannes (M)
Si le casque est débranché, le retour s'arrête avec un bandeau d'erreur ; la sortie vers le micro virtuel continue sans interruption.

### F7 - Soundboard

Objectif : jouer des sons locaux dans le micro virtuel.

**AC-F7-1** : Ajout de fichier (A+M)
Glisser un fichier WAV, MP3, OGG ou FLAC de 30 s ou moins sur la fenêtre, ou utiliser "Ajouter", crée un pad nommé d'après le fichier sans extension. Limite 16 pads : au-delà, message "Limite de 16 pads atteinte".

**AC-F7-2** : Déclenchement (A+M)
Cliquer un pad pendant que la voix est transmise ajoute le son au signal de "CABLE Output" sans couper la voix. Le premier échantillon du son est écrit dans la période de sortie suivante (test d'intégration sur la file de commandes).

**AC-F7-3** : Volumes et arrêt (A)
Chaque pad a un volume de -60 à +6 dB, il existe un volume global du soundboard et un bouton "Tout arrêter" ; cliquer un pad en cours de lecture le relance depuis le début.

**AC-F7-4** : Fichier invalide (A)
Un fichier non audio, corrompu, d'un format non pris en charge ou de plus de 30 s n'est pas ajouté, et un message nomme le fichier et la raison ; l'application continue de fonctionner.

**AC-F7-5** : Fichier déplacé (A)
Au lancement, un pad dont le fichier n'existe plus est grisé avec "Fichier introuvable" ; les autres pads restent utilisables.

**AC-F7-6** : Charge (M)
Déclencher 16 pads en moins d'une seconde pendant 1 minute ne fait pas monter le compteur d'underruns.

### F8 - Persistance des réglages

Objectif : retrouver sa configuration au lancement suivant.

**AC-F8-1** : Restauration (A+M)
Après fermeture et relance, les noms de périphériques, tous les réglages des effets et la liste des pads sont restaurés. Le retour micro reste désactivé (AC-F6-2) et le flux ne démarre pas tout seul.

**AC-F8-2** : Fichier corrompu (A)
Si config.json est illisible, l'application démarre avec les valeurs par défaut, renomme le fichier en config.json.bak et affiche un message.

**AC-F8-3** : Écriture atomique (A)
La sauvegarde écrit un fichier temporaire puis le renomme ; l'interruption de l'écriture laisse l'ancienne configuration valide.

**AC-F8-4** : Périphérique enregistré absent (A)
Si le périphérique enregistré n'existe plus, les périphériques par défaut Windows sont sélectionnés et un message l'indique.

## 5. Critères transverses (non fonctionnels)

Couverts par la tâche T10 : latence (AC-F1-3), stabilité (AC-F1-4), absence d'accès réseau (vérification manuelle dans RISKS_AND_TESTS.md, M5).
