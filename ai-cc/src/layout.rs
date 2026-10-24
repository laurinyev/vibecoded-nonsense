use crate::ast::{Field, Type};

pub fn align_up(value: usize, alignment: usize) -> usize {
    let alignment = alignment.max(1);
    value.div_ceil(alignment) * alignment
}

pub fn align(ty: &Type) -> usize {
    match ty {
        Type::Qualified(inner, _) => align(inner),
        Type::Bool | Type::Char | Type::SignedChar | Type::UnsignedChar => 1,
        Type::Short | Type::UnsignedShort => 2,
        Type::Int | Type::SignedInt | Type::UnsignedInt | Type::Float | Type::Enum(_, _) => 4,
        Type::Long
        | Type::UnsignedLong
        | Type::LongLong
        | Type::UnsignedLongLong
        | Type::Double
        | Type::Pointer(_) => 8,
        Type::LongDouble => 16,
        Type::Complex(inner) => align(inner),
        Type::Array(element, _, _) | Type::Vla(element, _) => align(element),
        Type::Struct(_, fields) | Type::Union(_, fields) => fields
            .iter()
            .map(|field| align(&field.ty))
            .max()
            .unwrap_or(1),
        _ => 4,
    }
}

pub fn size(ty: &Type) -> usize {
    match ty {
        Type::Qualified(inner, _) => size(inner),
        Type::Bool | Type::Char | Type::SignedChar | Type::UnsignedChar => 1,
        Type::Short | Type::UnsignedShort => 2,
        Type::Int | Type::SignedInt | Type::UnsignedInt | Type::Float | Type::Enum(_, _) => 4,
        Type::Long
        | Type::UnsignedLong
        | Type::LongLong
        | Type::UnsignedLongLong
        | Type::Double
        | Type::Pointer(_) => 8,
        Type::LongDouble => 16,
        Type::Complex(inner) => size(inner) * 2,
        Type::Array(element, Some(count), _) => size(element) * count,
        Type::Array(_, None, _) | Type::Vla(_, _) => 0,
        Type::Struct(_, fields) => struct_layout(fields).0,
        Type::Union(_, fields) => union_layout(fields).0,
        _ => 8,
    }
}

#[derive(Clone, Debug)]
pub struct FieldLayout {
    pub offset: usize,
    pub bit_offset: u32,
    pub width: Option<u32>,
    pub ty: Type,
}

pub fn struct_layout(fields: &[Field]) -> (usize, Vec<FieldLayout>) {
    let mut layouts = Vec::with_capacity(fields.len());
    let mut bit_position = 0usize;
    let mut alignment = 1usize;

    for field in fields {
        let field_alignment = align(&field.ty);
        alignment = alignment.max(field_alignment);

        match field.bit_width {
            Some(width) => {
                let unit_bits = size(&field.ty).max(1) * 8;
                if width == 0 {
                    bit_position = align_up(bit_position.div_ceil(8), field_alignment) * 8;
                    layouts.push(FieldLayout {
                        offset: bit_position / 8,
                        bit_offset: 0,
                        width: Some(0),
                        ty: field.ty.clone(),
                    });
                    continue;
                }

                let mut unit_start = bit_position / unit_bits * unit_bits;
                unit_start =
                    align_up(unit_start.div_ceil(8), field_alignment).min(usize::MAX / 8) * 8;
                if bit_position < unit_start {
                    bit_position = unit_start;
                }
                if bit_position - unit_start + width as usize > unit_bits {
                    bit_position = align_up(bit_position.div_ceil(8), field_alignment) * 8;
                    bit_position = align_up(bit_position.div_ceil(8), size(&field.ty)) * 8;
                    unit_start = bit_position;
                }
                layouts.push(FieldLayout {
                    offset: unit_start / 8,
                    bit_offset: (bit_position - unit_start) as u32,
                    width: Some(width),
                    ty: field.ty.clone(),
                });
                bit_position += width as usize;
            }
            None => {
                let byte_offset = align_up(bit_position.div_ceil(8), field_alignment);
                layouts.push(FieldLayout {
                    offset: byte_offset,
                    bit_offset: 0,
                    width: None,
                    ty: field.ty.clone(),
                });
                bit_position = (byte_offset + size(&field.ty)) * 8;
            }
        }
    }

    let size = align_up(bit_position.div_ceil(8), alignment);
    (size, layouts)
}

pub fn union_layout(fields: &[Field]) -> (usize, Vec<FieldLayout>) {
    let mut max_size = 0usize;
    let mut alignment = 1usize;
    let mut layouts = Vec::with_capacity(fields.len());
    for field in fields {
        let field_alignment = align(&field.ty);
        alignment = alignment.max(field_alignment);
        let field_size = size(&field.ty);
        max_size = max_size.max(field_size);
        layouts.push(FieldLayout {
            offset: 0,
            bit_offset: 0,
            width: field.bit_width,
            ty: field.ty.clone(),
        });
    }
    (align_up(max_size, alignment), layouts)
}

pub fn layout(ty: &Type) -> Option<(usize, Vec<FieldLayout>)> {
    match ty {
        Type::Qualified(inner, _) => layout(inner),
        Type::Struct(_, fields) => Some(struct_layout(fields)),
        Type::Union(_, fields) => Some(union_layout(fields)),
        _ => None,
    }
}

pub fn member(ty: &Type, index: usize) -> Option<FieldLayout> {
    let (_, layouts) = layout(ty)?;
    layouts.get(index).cloned()
}

pub fn field(ty: &Type, name: &str) -> Option<FieldLayout> {
    let fields = match ty {
        Type::Qualified(inner, _) => return field(inner, name),
        Type::Struct(_, fields) | Type::Union(_, fields) => fields,
        _ => return None,
    };
    let index = fields
        .iter()
        .position(|field| field.name.as_deref() == Some(name))?;
    member(ty, index)
}

pub fn struct_offset(fields: &[Field], index: usize) -> usize {
    struct_layout(fields)
        .1
        .get(index)
        .map(|field| field.offset)
        .unwrap_or(0)
}
