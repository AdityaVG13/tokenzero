//! Dimensional token accounting: a count carries its accounting class in the type.
//!
//! Token totals in different classes must never be combined implicitly -- adding
//! visible tokens to raw tokens, or billed-input to cached, is the bug class behind
//! per-event/percentage methodology errors. `Tok<C>` makes every such mix a compile
//! error; the only cross-class path is the audited [`Tok::cast`] escape hatch.

use core::fmt;
use core::iter::Sum;
use core::marker::PhantomData;
use core::ops::{Add, AddAssign, Sub, SubAssign};

mod sealed {
    pub trait Sealed {}
}

/// Accounting class marker. Sealed: the class set is part of the accounting
/// contract, and downstream crates must not mint classes that bypass it.
pub trait TokenClass:
    sealed::Sealed + Copy + Clone + Eq + Ord + core::hash::Hash + Default + 'static
{
    const NAME: &'static str;
}

macro_rules! token_classes {
    ($($(#[$doc:meta])* $name:ident => $label:literal),+ $(,)?) => {
        $(
            $(#[$doc])*
            #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
            pub struct $name;
            impl sealed::Sealed for $name {}
            impl TokenClass for $name {
                const NAME: &'static str = $label;
            }
        )+
    };
}

token_classes! {
    /// Tokens rendered into the model-facing transcript.
    Visible => "visible",
    /// Tokens of the underlying raw content before compaction.
    Raw => "raw",
    /// Input tokens billed at the full (uncached) rate.
    BilledIn => "billed_in",
    /// Output tokens billed by the provider.
    BilledOut => "billed_out",
    /// Input tokens served from a provider cache at the cached rate.
    Cached => "cached",
}

/// A token count in accounting class `C`.
///
/// Arithmetic is class-preserving; mixing classes fails to compile:
///
/// ```compile_fail
/// use tokenzero_core::token_classes::{Raw, Tok, Visible};
/// let visible = Tok::<Visible>::new(10);
/// let raw = Tok::<Raw>::new(20);
/// let _ = visible + raw;
/// ```
///
/// ```compile_fail
/// use tokenzero_core::token_classes::{BilledIn, Cached, Tok};
/// fn billed_total(n: Tok<BilledIn>) -> u64 { n.get() }
/// let cached = Tok::<Cached>::new(5);
/// billed_total(cached);
/// ```
///
/// Same-class arithmetic works as expected:
///
/// ```
/// use tokenzero_core::token_classes::{Tok, Visible};
/// let total: Tok<Visible> = Tok::new(30) + Tok::new(12);
/// assert_eq!(total.get(), 42);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tok<C: TokenClass> {
    count: u64,
    class: PhantomData<fn() -> C>,
}

impl<C: TokenClass> Tok<C> {
    pub const ZERO: Self = Self::new(0);

    #[must_use]
    pub const fn new(count: u64) -> Self {
        Self {
            count,
            class: PhantomData,
        }
    }

    /// Lossless on every supported target: counts originate as `usize` in the
    /// render/measurement paths and `usize` never exceeds `u64` here.
    #[must_use]
    pub const fn from_usize(count: usize) -> Self {
        Self::new(count as u64)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.count
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.count == 0
    }

    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.count.checked_add(rhs.count) {
            Some(count) => Some(Self::new(count)),
            None => None,
        }
    }

    #[must_use]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self::new(self.count.saturating_add(rhs.count))
    }

    #[must_use]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self::new(self.count.saturating_sub(rhs.count))
    }

    /// The one legal cross-class conversion. Every call site is an audit point:
    /// reclassification changes what a number *means*, so it must be visible in
    /// review rather than smuggled through arithmetic.
    #[must_use]
    pub const fn cast<D: TokenClass>(self) -> Tok<D> {
        Tok::new(self.count)
    }
}

impl<C: TokenClass> fmt::Debug for Tok<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tok<{}>({})", C::NAME, self.count)
    }
}

impl<C: TokenClass> fmt::Display for Tok<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.count, C::NAME)
    }
}

impl<C: TokenClass> Add for Tok<C> {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.count + rhs.count)
    }
}

impl<C: TokenClass> AddAssign for Tok<C> {
    fn add_assign(&mut self, rhs: Self) {
        self.count += rhs.count;
    }
}

impl<C: TokenClass> Sub for Tok<C> {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.count - rhs.count)
    }
}

impl<C: TokenClass> SubAssign for Tok<C> {
    fn sub_assign(&mut self, rhs: Self) {
        self.count -= rhs.count;
    }
}

impl<C: TokenClass> Sum for Tok<C> {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

impl<'a, C: TokenClass> Sum<&'a Tok<C>> for Tok<C> {
    fn sum<I: Iterator<Item = &'a Tok<C>>>(iter: I) -> Self {
        iter.copied().sum()
    }
}

/// Class-typed front door to [`crate::tokens::savings_ratio`]: the ratio is only
/// meaningful with raw in the numerator's baseline and visible as the spend, and
/// the types now enforce which argument is which.
#[must_use]
pub fn savings_ratio_typed(raw: Tok<Raw>, visible: Tok<Visible>) -> f64 {
    crate::tokens::savings_ratio(raw.get() as usize, visible.get() as usize)
}

#[cfg(test)]
mod token_class_tests {
    use super::*;

    #[test]
    fn same_class_arithmetic_and_sum() {
        let mut visible = Tok::<Visible>::new(40);
        visible += Tok::new(2);
        assert_eq!(visible.get(), 42);
        assert_eq!((visible - Tok::new(2)).get(), 40);

        let parts = [Tok::<Raw>::new(1), Tok::new(2), Tok::new(3)];
        let total: Tok<Raw> = parts.iter().sum();
        assert_eq!(total, Tok::new(6));
    }

    #[test]
    fn saturating_and_checked_edges() {
        let max = Tok::<BilledIn>::new(u64::MAX);
        assert_eq!(max.checked_add(Tok::new(1)), None);
        assert_eq!(max.saturating_add(Tok::new(1)), max);
        assert_eq!(Tok::<BilledIn>::ZERO.saturating_sub(Tok::new(9)).get(), 0);
    }

    #[test]
    fn cast_is_explicit_and_value_preserving() {
        let raw = Tok::<Raw>::new(641);
        let reclassified: Tok<Visible> = raw.cast();
        assert_eq!(reclassified.get(), 641);
    }

    #[test]
    fn display_and_debug_carry_the_class_name() {
        let cached = Tok::<Cached>::new(7);
        assert_eq!(cached.to_string(), "7 cached");
        assert_eq!(format!("{cached:?}"), "Tok<cached>(7)");
        assert_eq!(Tok::<BilledOut>::new(3).to_string(), "3 billed_out");
    }

    #[test]
    fn typed_savings_ratio_matches_untyped() {
        let raw = Tok::<Raw>::new(641);
        let visible = Tok::<Visible>::new(331);
        let expected = crate::tokens::savings_ratio(641, 331);
        assert!((savings_ratio_typed(raw, visible) - expected).abs() < f64::EPSILON);
    }
}
