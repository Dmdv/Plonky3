use alloc::vec;
use alloc::vec::Vec;
use core::fmt::{Debug, Display};
use core::hash::Hash;
use core::iter::{Product, Sum};
use core::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};
use core::slice;

use itertools::Itertools;
use num_bigint::BigUint;
use num_traits::One;
use nums::{Factorizer, FactorizerFromSplitter, MillerRabin, PollardRho};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::exponentiation::exp_u64_by_squaring;
use crate::packed::{PackedField, PackedValue};
use crate::Packable;

/// A generalization of `Field` which permits things like
/// - an actual field element
/// - a symbolic expression which would evaluate to a field element
/// - an array of field elements
pub trait AbstractField:
    Sized
    + Default
    + Clone
    + Add<Output = Self>
    + AddAssign
    + Sub<Output = Self>
    + SubAssign
    + Neg<Output = Self>
    + Mul<Output = Self>
    + MulAssign
    + Sum
    + Product
    + Debug
{
    type F: Field;

    const ZERO: Self;
    const ONE: Self;
    const TWO: Self;
    const NEG_ONE: Self;

    fn from_f(f: Self::F) -> Self;

    /// Convert from a `bool`.
    fn from_bool(b: bool) -> Self;

    /// Convert from a canonical `u8`.
    ///
    /// If the input is not canonical, i.e. if it exceeds the field's characteristic, then the
    /// behavior is undefined.
    fn from_canonical_u8(n: u8) -> Self;

    /// Convert from a canonical `u16`.
    ///
    /// If the input is not canonical, i.e. if it exceeds the field's characteristic, then the
    /// behavior is undefined.
    fn from_canonical_u16(n: u16) -> Self;

    /// Convert from a canonical `u32`.
    ///
    /// If the input is not canonical, i.e. if it exceeds the field's characteristic, then the
    /// behavior is undefined.
    fn from_canonical_u32(n: u32) -> Self;

    /// Convert from a canonical `u64`.
    ///
    /// If the input is not canonical, i.e. if it exceeds the field's characteristic, then the
    /// behavior is undefined.
    fn from_canonical_u64(n: u64) -> Self;

    /// Convert from a canonical `usize`.
    ///
    /// If the input is not canonical, i.e. if it exceeds the field's characteristic, then the
    /// behavior is undefined.
    fn from_canonical_usize(n: usize) -> Self;

    fn from_wrapped_u32(n: u32) -> Self;
    fn from_wrapped_u64(n: u64) -> Self;

    #[must_use]
    fn double(&self) -> Self {
        self.clone() + self.clone()
    }

    #[must_use]
    fn square(&self) -> Self {
        self.clone() * self.clone()
    }

    #[must_use]
    fn cube(&self) -> Self {
        self.square() * self.clone()
    }

    /// Exponentiation by a `u64` power.
    ///
    /// The default implementation calls `exp_u64_generic`, which by default performs exponentiation
    /// by squaring. Rather than override this method, it is generally recommended to have the
    /// concrete field type override `exp_u64_generic`, so that any optimizations will apply to all
    /// abstract fields.
    #[must_use]
    #[inline]
    fn exp_u64(&self, power: u64) -> Self {
        Self::F::exp_u64_generic(self.clone(), power)
    }

    #[must_use]
    #[inline(always)]
    fn exp_const_u64<const POWER: u64>(&self) -> Self {
        match POWER {
            0 => Self::ONE,
            1 => self.clone(),
            2 => self.square(),
            3 => self.cube(),
            4 => self.square().square(),
            5 => self.square().square() * self.clone(),
            6 => self.square().cube(),
            7 => {
                let x2 = self.square();
                let x3 = x2.clone() * self.clone();
                let x4 = x2.square();
                x3 * x4
            }
            _ => self.exp_u64(POWER),
        }
    }

    #[must_use]
    fn exp_power_of_2(&self, power_log: usize) -> Self {
        let mut res = self.clone();
        for _ in 0..power_log {
            res = res.square();
        }
        res
    }

    #[must_use]
    fn powers(&self) -> Powers<Self> {
        self.shifted_powers(Self::ONE)
    }

    fn shifted_powers(&self, start: Self) -> Powers<Self> {
        Powers {
            base: self.clone(),
            current: start,
        }
    }

    fn powers_packed<P: PackedField<Scalar = Self>>(&self) -> PackedPowers<Self, P> {
        self.shifted_powers_packed(Self::ONE)
    }

    fn shifted_powers_packed<P: PackedField<Scalar = Self>>(
        &self,
        start: Self,
    ) -> PackedPowers<Self, P> {
        let mut current = P::from_f(start);
        let slice = current.as_slice_mut();
        for i in 1..P::WIDTH {
            slice[i] = slice[i - 1].clone() * self.clone();
        }

        PackedPowers {
            multiplier: P::from_f(self.clone()).exp_u64(P::WIDTH as u64),
            current,
        }
    }

    fn dot_product<const N: usize>(u: &[Self; N], v: &[Self; N]) -> Self {
        u.iter().zip(v).map(|(x, y)| x.clone() * y.clone()).sum()
    }

    fn try_div<Rhs>(self, rhs: Rhs) -> Option<<Self as Mul<Rhs>>::Output>
    where
        Rhs: Field,
        Self: Mul<Rhs>,
    {
        rhs.try_inverse().map(|inv| self * inv)
    }

    /// Allocates a vector of zero elements of length `len`. Many operating systems zero pages
    /// before assigning them to a userspace process. In that case, our process should not need to
    /// write zeros, which would be redundant. However, the compiler may not always recognize this.
    ///
    /// In particular, `vec![Self::ZERO; len]` appears to result in redundant userspace zeroing.
    /// This is the default implementation, but implementors may wish to provide their own
    /// implementation which transmutes something like `vec![0u32; len]`.
    #[inline]
    fn zero_vec(len: usize) -> Vec<Self> {
        vec![Self::ZERO; len]
    }
}

/// An element of a finite field.
pub trait Field:
    AbstractField<F = Self>
    + Packable
    + 'static
    + Copy
    + Div<Self, Output = Self>
    + Eq
    + Hash
    + Send
    + Sync
    + Display
    + Serialize
    + DeserializeOwned
{
    type Packing: PackedField<Scalar = Self>;

    /// A generator of this field's entire multiplicative group.
    const GENERATOR: Self;

    fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }

    fn is_one(&self) -> bool {
        *self == Self::ONE
    }

    /// self * 2^exp
    #[must_use]
    #[inline]
    fn mul_2exp_u64(&self, exp: u64) -> Self {
        *self * Self::TWO.exp_u64(exp)
    }

    /// self / 2^exp
    #[must_use]
    #[inline]
    fn div_2exp_u64(&self, exp: u64) -> Self {
        *self / Self::TWO.exp_u64(exp)
    }

    /// Exponentiation by a `u64` power. This is similar to `exp_u64`, but more general in that it
    /// can be used with `AbstractField`s, not just this concrete field.
    ///
    /// The default implementation uses naive square and multiply. Implementations may want to
    /// override this and handle certain powers with more optimal addition chains.
    #[must_use]
    #[inline]
    fn exp_u64_generic<AF: AbstractField<F = Self>>(val: AF, power: u64) -> AF {
        exp_u64_by_squaring(val, power)
    }

    /// The multiplicative inverse of this field element, if it exists.
    ///
    /// NOTE: The inverse of `0` is undefined and will return `None`.
    #[must_use]
    fn try_inverse(&self) -> Option<Self>;

    #[must_use]
    fn inverse(&self) -> Self {
        self.try_inverse().expect("Tried to invert zero")
    }

    /// Computes input/2.
    /// Should be overwritten by most field implementations to use bitshifts.
    /// Will error if the field characteristic is 2.
    #[must_use]
    fn halve(&self) -> Self {
        let half = Self::TWO
            .try_inverse()
            .expect("Cannot divide by 2 in fields with characteristic 2");
        *self * half
    }

    fn order() -> BigUint;

    /// A list of (factor, exponent) pairs.
    fn multiplicative_group_factors() -> Vec<(BigUint, usize)> {
        let primality_test = MillerRabin { error_bits: 128 };
        let composite_splitter = PollardRho;
        let factorizer = FactorizerFromSplitter {
            primality_test,
            composite_splitter,
        };
        let n = Self::order() - BigUint::one();
        factorizer.factor_counts(&n)
    }

    #[inline]
    fn bits() -> usize {
        Self::order().bits() as usize
    }
}

pub trait PrimeField: Field + Ord {
    fn as_canonical_biguint(&self) -> BigUint;
}

/// A prime field of order less than `2^64`.
pub trait PrimeField64: PrimeField {
    const ORDER_U64: u64;

    /// Return the representative of `value` that is less than `ORDER_U64`.
    fn as_canonical_u64(&self) -> u64;
}

/// A prime field of order less than `2^32`.
pub trait PrimeField32: PrimeField64 {
    const ORDER_U32: u32;

    /// Return the representative of `value` that is less than `ORDER_U32`.
    fn as_canonical_u32(&self) -> u32;
}

pub trait AbstractExtensionField<Base: AbstractField>:
    AbstractField
    + From<Base>
    + Add<Base, Output = Self>
    + AddAssign<Base>
    + Sub<Base, Output = Self>
    + SubAssign<Base>
    + Mul<Base, Output = Self>
    + MulAssign<Base>
{
    const D: usize;

    fn from_base(b: Base) -> Self;

    /// Suppose this field extension is represented by the quotient
    /// ring B[X]/(f(X)) where B is `Base` and f is an irreducible
    /// polynomial of degree `D`. This function takes a slice `bs` of
    /// length at exactly D, and constructs the field element
    /// \sum_i bs[i] * X^i.
    ///
    /// NB: The value produced by this function fundamentally depends
    /// on the choice of irreducible polynomial f. Care must be taken
    /// to ensure portability if these values might ever be passed to
    /// (or rederived within) another compilation environment where a
    /// different f might have been used.
    fn from_base_slice(bs: &[Base]) -> Self;

    /// Similar to `core:array::from_fn`, with the same caveats as
    /// `from_base_slice`.
    fn from_base_fn<F: FnMut(usize) -> Base>(f: F) -> Self;
    fn from_base_iter<I: Iterator<Item = Base>>(iter: I) -> Self;

    /// Suppose this field extension is represented by the quotient
    /// ring B[X]/(f(X)) where B is `Base` and f is an irreducible
    /// polynomial of degree `D`. This function takes a field element
    /// \sum_i bs[i] * X^i and returns the coefficients as a slice
    /// `bs` of length at most D containing, from lowest degree to
    /// highest.
    ///
    /// NB: The value produced by this function fundamentally depends
    /// on the choice of irreducible polynomial f. Care must be taken
    /// to ensure portability if these values might ever be passed to
    /// (or rederived within) another compilation environment where a
    /// different f might have been used.
    fn as_base_slice(&self) -> &[Base];

    /// Suppose this field extension is represented by the quotient
    /// ring B[X]/(f(X)) where B is `Base` and f is an irreducible
    /// polynomial of degree `D`. This function returns the field
    /// element `X^exponent` if `exponent < D` and panics otherwise.
    /// (The fact that f is not known at the point that this function
    /// is defined prevents implementing exponentiation of higher
    /// powers since the reduction cannot be performed.)
    ///
    /// NB: The value produced by this function fundamentally depends
    /// on the choice of irreducible polynomial f. Care must be taken
    /// to ensure portability if these values might ever be passed to
    /// (or rederived within) another compilation environment where a
    /// different f might have been used.
    fn monomial(exponent: usize) -> Self {
        assert!(exponent < Self::D, "requested monomial of too high degree");
        let mut vec = vec![Base::ZERO; Self::D];
        vec[exponent] = Base::ONE;
        Self::from_base_slice(&vec)
    }
}

pub trait ExtensionField<Base: Field>: Field + AbstractExtensionField<Base> {
    type ExtensionPacking: AbstractExtensionField<Base::Packing, F = Self>
        + 'static
        + Copy
        + Send
        + Sync;

    fn is_in_basefield(&self) -> bool {
        self.as_base_slice()[1..].iter().all(Field::is_zero)
    }
    fn as_base(&self) -> Option<Base> {
        if self.is_in_basefield() {
            Some(self.as_base_slice()[0])
        } else {
            None
        }
    }

    fn ext_powers_packed(&self) -> impl Iterator<Item = Self::ExtensionPacking> {
        let powers = self.powers().take(Base::Packing::WIDTH + 1).collect_vec();
        // Transpose first WIDTH powers
        let current = Self::ExtensionPacking::from_base_fn(|i| {
            Base::Packing::from_fn(|j| powers[j].as_base_slice()[i])
        });
        // Broadcast self^WIDTH
        let multiplier = Self::ExtensionPacking::from_base_fn(|i| {
            Base::Packing::from(powers[Base::Packing::WIDTH].as_base_slice()[i])
        });

        core::iter::successors(Some(current), move |&current| Some(current * multiplier))
    }
}

impl<F: Field> ExtensionField<F> for F {
    type ExtensionPacking = F::Packing;
}

impl<AF: AbstractField> AbstractExtensionField<AF> for AF {
    const D: usize = 1;

    fn from_base(b: AF) -> Self {
        b
    }

    fn from_base_slice(bs: &[AF]) -> Self {
        assert_eq!(bs.len(), 1);
        bs[0].clone()
    }

    fn from_base_iter<I: Iterator<Item = AF>>(mut iter: I) -> Self {
        iter.next().unwrap()
    }

    fn from_base_fn<F: FnMut(usize) -> AF>(mut f: F) -> Self {
        f(0)
    }

    fn as_base_slice(&self) -> &[AF] {
        slice::from_ref(self)
    }
}

/// A field which supplies information like the two-adicity of its multiplicative group, and methods
/// for obtaining two-adic generators.
pub trait TwoAdicField: Field {
    /// The number of factors of two in this field's multiplicative group.
    const TWO_ADICITY: usize;

    /// Returns a generator of the multiplicative group of order `2^bits`.
    /// Assumes `bits <= TWO_ADICITY`, otherwise the result is undefined.
    #[must_use]
    fn two_adic_generator(bits: usize) -> Self;
}

/// An iterator over the powers of a certain base element `b`: `b^0, b^1, b^2, ...`.
#[derive(Clone, Debug)]
pub struct Powers<F> {
    pub base: F,
    pub current: F,
}

impl<AF: AbstractField> Iterator for Powers<AF> {
    type Item = AF;

    fn next(&mut self) -> Option<AF> {
        let result = self.current.clone();
        self.current *= self.base.clone();
        Some(result)
    }
}

/// like `Powers`, but packed into `PackedField` elements
#[derive(Clone, Debug)]
pub struct PackedPowers<F, P: PackedField<Scalar = F>> {
    // base ** P::WIDTH
    pub multiplier: P,
    pub current: P,
}

impl<AF: AbstractField, P: PackedField<Scalar = AF>> Iterator for PackedPowers<AF, P> {
    type Item = P;

    fn next(&mut self) -> Option<P> {
        let result = self.current;
        self.current *= self.multiplier;
        Some(result)
    }
}
