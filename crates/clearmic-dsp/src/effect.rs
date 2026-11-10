/// Un effet traite un bloc d'echantillons f32 entrelaces, en place.
///
/// `process` est appele depuis le rappel audio : aucune allocation, aucun verrou.
pub trait Effect: Send {
    fn process(&mut self, buf: &mut [f32], channels: usize);
}
