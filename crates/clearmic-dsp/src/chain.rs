use crate::effect::Effect;

/// Suite d'effets appliques dans l'ordre sur un meme bloc.
#[derive(Default)]
pub struct Chain {
    effects: Vec<Box<dyn Effect>>,
}

impl Chain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, effect: Box<dyn Effect>) {
        self.effects.push(effect);
    }
}

impl Effect for Chain {
    fn process(&mut self, buf: &mut [f32], channels: usize) {
        for e in &mut self.effects {
            e.process(buf, channels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mul(f32);
    impl Effect for Mul {
        fn process(&mut self, buf: &mut [f32], _channels: usize) {
            buf.iter_mut().for_each(|s| *s *= self.0);
        }
    }

    #[test]
    fn applies_in_order() {
        let mut c = Chain::new();
        c.push(Box::new(Mul(2.0)));
        c.push(Box::new(Mul(3.0)));
        let mut b = [1.0f32, -1.0];
        c.process(&mut b, 1);
        assert_eq!(b, [6.0, -6.0]);
    }

    #[test]
    fn empty_chain_is_transparent() {
        let mut c = Chain::new();
        let mut b = [0.25f32, -0.5];
        c.process(&mut b, 2);
        assert_eq!(b, [0.25, -0.5]);
    }
}
