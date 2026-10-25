use crate::{ast::*, layout};

pub(crate) fn integer_constant(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(value) => Some(*value),
        Expr::Char(value) => Some(*value as i64),
        Expr::SizeofType(ty) if complete_object_type(ty) => i64::try_from(layout::size(ty)).ok(),
        Expr::Cast(ty, value) if integer_type(ty) => {
            let value = match &**value {
                Expr::Float(value, _) => *value as i64,
                _ => integer_constant(value)?,
            };
            Some(cast_integer(value, ty))
        }
        Expr::Unary(UnOp::Pos, value) => integer_constant(value),
        Expr::Unary(UnOp::Neg, value) => integer_constant(value)?.checked_neg(),
        Expr::Unary(UnOp::Not, value) => Some((integer_constant(value)? == 0) as i64),
        Expr::Unary(UnOp::BitNot, value) => Some(!integer_constant(value)?),
        Expr::Binary(BinOp::And, left, right) => {
            if integer_constant(left)? == 0 {
                Some(0)
            } else {
                Some((integer_constant(right)? != 0) as i64)
            }
        }
        Expr::Binary(BinOp::Or, left, right) => {
            if integer_constant(left)? != 0 {
                Some(1)
            } else {
                Some((integer_constant(right)? != 0) as i64)
            }
        }
        Expr::Binary(BinOp::Comma, _, _) => None,
        Expr::Binary(op, left, right) => {
            let (left, right) = (integer_constant(left)?, integer_constant(right)?);
            match op {
                BinOp::Add => left.checked_add(right),
                BinOp::Sub => left.checked_sub(right),
                BinOp::Mul => left.checked_mul(right),
                BinOp::Div => left.checked_div(right),
                BinOp::Rem => left.checked_rem(right),
                BinOp::Shl => {
                    let shift = u32::try_from(right).ok()?;
                    if left < 0 || shift >= 64 {
                        return None;
                    }
                    i64::try_from((left as i128) << shift).ok()
                }
                BinOp::Shr => {
                    let shift = u32::try_from(right).ok()?;
                    (shift < 64).then(|| left >> shift)
                }
                BinOp::BitAnd => Some(left & right),
                BinOp::BitOr => Some(left | right),
                BinOp::BitXor => Some(left ^ right),
                BinOp::Eq => Some((left == right) as i64),
                BinOp::Ne => Some((left != right) as i64),
                BinOp::Lt => Some((left < right) as i64),
                BinOp::Le => Some((left <= right) as i64),
                BinOp::Gt => Some((left > right) as i64),
                BinOp::Ge => Some((left >= right) as i64),
                BinOp::And | BinOp::Or | BinOp::Comma => unreachable!(),
            }
        }
        Expr::Conditional(condition, yes, no) => {
            if integer_constant(condition)? != 0 {
                integer_constant(yes)
            } else {
                integer_constant(no)
            }
        }
        _ => None,
    }
}

fn integer_type(ty: &Type) -> bool {
    match ty {
        Type::Qualified(inner, _) => integer_type(inner),
        Type::Bool
        | Type::Char
        | Type::SignedChar
        | Type::UnsignedChar
        | Type::Short
        | Type::UnsignedShort
        | Type::Int
        | Type::SignedInt
        | Type::UnsignedInt
        | Type::Long
        | Type::UnsignedLong
        | Type::LongLong
        | Type::UnsignedLongLong
        | Type::Enum(_, _) => true,
        _ => false,
    }
}

fn cast_integer(value: i64, ty: &Type) -> i64 {
    match ty {
        Type::Qualified(inner, _) => cast_integer(value, inner),
        Type::Bool => i64::from(value != 0),
        Type::Char | Type::SignedChar => value as i8 as i64,
        Type::UnsignedChar => value as u8 as i64,
        Type::Short => value as i16 as i64,
        Type::UnsignedShort => value as u16 as i64,
        Type::Int | Type::SignedInt | Type::Enum(_, _) => value as i32 as i64,
        Type::UnsignedInt => value as u32 as i64,
        Type::UnsignedLong | Type::UnsignedLongLong => value as u64 as i64,
        _ => value,
    }
}

fn complete_object_type(ty: &Type) -> bool {
    match ty {
        Type::Qualified(inner, _) => complete_object_type(inner),
        Type::Array(inner, Some(count), _) => *count > 0 && complete_object_type(inner),
        Type::Pointer(_) => true,
        Type::Struct(_, fields) | Type::Union(_, fields) => !fields.is_empty(),
        Type::Enum(_, values) => !values.is_empty(),
        Type::Void | Type::Function(_, _, _, _) | Type::Array(_, None, _) | Type::Vla(_, _) => {
            false
        }
        _ => true,
    }
}
