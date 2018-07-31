// Copyright 2018 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate. If not, see <http://www.gnu.org/licenses/>.

// TODO: Deglob
use super::super::*;
use super::{ConvertibleToWasm, Environment};
use parity_wasm::elements::FunctionType;
use parity_wasm::elements::ValueType;
use rstd::collections::btree_map::BTreeMap;
use runtime_primitives::traits::Zero;
use sandbox::ReturnValue;
use sandbox::TypedValue;

#[macro_export]
macro_rules! convert_args {
	() => ([]);
	( $( $t:ty ),* ) => ( vec![ $( { use $crate::vm::env_def::ConvertibleToWasm; <$t>::VALUE_TYPE }, )* ] );
}

#[macro_export]
macro_rules! gen_signature {
	( ( $( $params: ty ),* ) ) => (
		{
			FunctionType::new(convert_args!($($params),*), None)
		}
	);

	( ( $( $params: ty ),* ) -> $returns: ty ) => (
		{
			FunctionType::new(convert_args!($($params),*), Some({
				use $crate::vm::env_def::ConvertibleToWasm; <$returns>::VALUE_TYPE
			}))
		}
	);
}

#[test]
fn macro_gen_signature() {
	assert_eq!(
		gen_signature!((i32)),
		FunctionType::new(vec![ValueType::I32], None),
	);

	assert_eq!(
		gen_signature!( (i32, u32) -> u32 ),
		FunctionType::new(vec![ValueType::I32, ValueType::I32], Some(ValueType::I32)),
	);
}

/// Unmarshall arguments and then execute `body` expression and return its result.
macro_rules! unmarshall_then_body {
	( $body:tt, $ctx:ident, $args_iter:ident, $( $names:ident : $params:ty ),* ) => ({
		$(
			let $names : <$params as $crate::vm::env_def::ConvertibleToWasm>::NativeType =
				$args_iter.next()
					.and_then(|v| <$params as $crate::vm::env_def::ConvertibleToWasm>::from_typed_value(v.clone()))
					.expect("TODO"); // TODO
		)*
		$body
	})
}

#[test]
fn macro_unmarshall_then_body() {
	let args = vec![TypedValue::I32(5), TypedValue::I32(3)];
	let mut args = args.iter();

	let ctx: &mut u32 = &mut 0;

	let r = unmarshall_then_body!(
		{
			*ctx = a + b;
			a * b
		},
		ctx,
		args,
		a: u32,
		b: u32
	);

	assert_eq!(*ctx, 8);
	assert_eq!(r, 15);
}

/// Since we can't specify the type of closure directly at binding site:
///
/// ```rust,ignore
/// let f: FnOnce() -> Result<<u32 as ConvertibleToWasm>::NativeType, _> = || { /* ... */ };
/// ```
///
/// we use this function to constrain the type of the closure.
#[inline(always)]
pub fn constrain_closure<R, F>(f: F) -> F
where
	F: FnOnce() -> Result<R, sandbox::HostError>,
{
	f
}

#[macro_export]
macro_rules! unmarshall_then_body_then_marshall {
	( $args_iter:ident, $ctx:ident, ( $( $names:ident : $params:ty ),* ) -> $returns:ty => $body:tt ) => ({
		let body = $crate::vm::env_def::macros::constrain_closure::<
			<$returns as $crate::vm::env_def::ConvertibleToWasm>::NativeType, _
		>(|| {
			unmarshall_then_body!($body, $ctx, $args_iter, $( $names : $params ),*)
		});
		let r = body()?;
		return Ok(ReturnValue::Value({ use $crate::vm::env_def::ConvertibleToWasm; r.to_typed_value() }))
	});
	( $args_iter:ident, $ctx:ident, ( $( $names:ident : $params:ty ),* ) => $body:tt ) => ({
		let body = $crate::vm::env_def::macros::constrain_closure::<(), _>(|| {
			unmarshall_then_body!($body, $ctx, $args_iter, $( $names : $params ),*)
		});
		body()?;
		return Ok($crate::sandbox::ReturnValue::Unit)
	})
}

#[test]
fn macro_unmarshall_then_body_then_marshall_value_or_trap() {
	fn test_value(
		_ctx: &mut u32,
		args: &[sandbox::TypedValue],
	) -> Result<ReturnValue, sandbox::HostError> {
		let mut args = args.iter();
		unmarshall_then_body_then_marshall!(
			args,
			_ctx,
			(a: u32, b: u32) -> u32 => {
				if b == 0 {
					Err(sandbox::HostError)
				} else {
					Ok(a / b)
				}
			}
		)
	}

	let ctx = &mut 0;
	assert_eq!(
		test_value(ctx, &[TypedValue::I32(15), TypedValue::I32(3)]).unwrap(),
		ReturnValue::Value(TypedValue::I32(5)),
	);
	assert!(test_value(ctx, &[TypedValue::I32(15), TypedValue::I32(0)]).is_err());
}

#[test]
fn macro_unmarshall_then_body_then_marshall_unit() {
	fn test_unit(
		ctx: &mut u32,
		args: &[sandbox::TypedValue],
	) -> Result<ReturnValue, sandbox::HostError> {
		let mut args = args.iter();
		unmarshall_then_body_then_marshall!(
			args,
			ctx,
			(a: u32, b: u32) => {
				*ctx = a + b;
				Ok(())
			}
		)
	}

	let ctx = &mut 0;
	let result = test_unit(ctx, &[TypedValue::I32(2), TypedValue::I32(3)]).unwrap();
	assert_eq!(result, ReturnValue::Unit);
	assert_eq!(*ctx, 5);
}

#[macro_export]
macro_rules! define_func {
	( < E: $ext_ty:tt > $name:ident ( $ctx: ident, $($names:ident : $params:ty),*) $(-> $returns:ty)* => $body:tt ) => {
		fn $name< E: $ext_ty >(
			$ctx: &mut $crate::vm::Runtime<E>,
			args: &[$crate::sandbox::TypedValue],
		) -> Result<sandbox::ReturnValue, sandbox::HostError> {
			#[allow(unused)]
			let mut args = args.iter();

			unmarshall_then_body_then_marshall!(
				args,
				$ctx,
				( $( $names : $params ),* ) $( -> $returns )* => $body
			)
		}
	};
}

#[test]
fn macro_define_func() {
	define_func!( <E: Ext> ext_gas (_ctx, amount: u32) => {
		let amount = <<<E as Ext>::T as Trait>::Gas as As<u32>>::sa(amount);
		if !amount.is_zero() {
			Ok(())
		} else {
			Err(sandbox::HostError)
		}
	});
	let _f: fn(&mut Runtime<::vm::tests::MockExt>, &[sandbox::TypedValue])
		-> Result<sandbox::ReturnValue, sandbox::HostError> = ext_gas::<::vm::tests::MockExt>;
}

macro_rules! define_env {
	( < E: $ext_ty:tt > ,  $( $name:ident ( $ctx:ident, $( $names:ident : $params:ty ),* ) $( -> $returns:ty )* => $body:tt , )* ) => {
		pub fn init_env<E: Ext>() -> Environment<E> {
			let mut env = Environment {
				funcs: BTreeMap::new(),
			};

			$(
				env.funcs.insert(
					stringify!( $name ).to_string(),
					ExtFunc {
						func_type: gen_signature!( ( $( $params ),* ) $( -> $returns )* ),
						f: {
							define_func!(
								< E: $ext_ty > $name ( $ctx, $( $names : $params ),* ) $( -> $returns )* => $body
							);
							$name::<E>
						},
					},
				);
			)*

			env
		}
	};
}

#[test]
fn macro_define_env() {
	define_env!(<E: Ext>,
		ext_gas( ctx, amount: u32 ) => {
			let amount = <<<E as Ext>::T as Trait>::Gas as As<u32>>::sa(amount);
			if !amount.is_zero() {
				Ok(())
			} else {
				Err(sandbox::HostError)
			}
		},
	);
}