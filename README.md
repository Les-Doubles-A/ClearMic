# ClearMic

Dépôt GitHub : **https://github.com/Les-Doubles-A/ClearMic**

Application Windows qui capte le micro, nettoie et mixe la voix en temps réel avec une latence minimale, puis l'envoie vers un micro virtuel utilisé par Discord, OBS, Teams ou un jeu. Cadrage complet dans `product/`.

<!-- installeur-release -->
## Installation

Téléchargez **`ClearMic-Setup-0.1.1.exe`** depuis la [dernière release](https://github.com/Les-Doubles-A/ClearMic/releases/latest), puis lancez-le.

L'installation se fait pour l'utilisateur courant, sans droits administrateur (par défaut dans `%LOCALAPPDATA%\Programs\ClearMic`), avec un raccourci dans le menu Démarrer et une désinstallation depuis les Paramètres Windows.

L'installeur n'est pas signé : si Windows SmartScreen l'affiche, cliquez sur « Informations complémentaires » puis « Exécuter quand même ».

### Recréer l'installeur

Compiler l'application (`cargo build --release`), puis lancer [Inno Setup 6](https://jrsoftware.org/isinfo.php) : `iscc installer\ClearMic.iss`. Le setup est créé dans `installer\Output\`.
