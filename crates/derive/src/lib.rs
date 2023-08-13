use convert_case::{Case, Casing};
use proc_macro::{self, TokenStream};
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident, Type};

#[proc_macro_derive(ToJs, attributes(js_type))]
pub fn derive_to_js(input: TokenStream) -> TokenStream {
  let DeriveInput {
    ident: self_name,
    data,
    ..
  } = parse_macro_input!(input);
  let register = Ident::new(
    &format!("register_{}", self_name.to_string().to_case(Case::Snake)),
    Span::call_site(),
  );

  let output = match data {
    Data::Struct(s) => {
      let mut js: Vec<proc_macro2::TokenStream> = Vec::new();
      match &s.fields {
        Fields::Named(_) => {
          for field in s.fields.iter() {
            let name = field.ident.as_ref().unwrap();
            let ty = if let Some(attr) = field
              .attrs
              .iter()
              .find(|attr| attr.path.is_ident("js_type"))
            {
              let ty: Type = attr.parse_args().unwrap();
              ty
            } else {
              field.ty.clone()
            };

            js.push(quote! {
              let name = stringify!(#name).to_case(Case::Camel);
              let offset = (std::ptr::addr_of!((*p).#name) as *const u8).offset_from(u8_ptr) as usize;
              let getter = <#ty>::js_getter(offset);
              let setter = <#ty>::js_setter(offset, "value");
              let type_name = <#ty>::ty();
              js.push_str(&format!(
  r#"
  get {name}(): {type_name} {{
    return {getter};
  }}

  set {name}(value: {type_name}): void {{
    {setter};
  }}
"#,
                name = name,
                getter = getter,
                setter = setter,
                type_name = type_name
              ));
            });
          }
        }
        _ => todo!(),
      }

      quote! {
        #[automatically_derived]
        impl ToJs for #self_name {
          fn to_js() -> String {
            use convert_case::{Case, Casing};
            let c = std::mem::MaybeUninit::uninit();
            let p: *const #self_name = c.as_ptr();
            let u8_ptr = p as *const u8;
            let mut js = String::new();
            let size = std::mem::size_of::<#self_name>();

            js.push_str(&format!(
      r#"export class {name} {{
  addr: number;

  constructor(addr?: number) {{
    this.addr = addr ?? binding.alloc({size});
  }}

  static get(addr: number): {name} {{
    return new {name}(addr);
  }}

  static set(addr: number, value: {name}): void {{
    copy(value.addr, addr, {size});
  }}
"#,
              name = stringify!(#self_name),
              size = size,
            ));

            unsafe {
              #(#js);*;
            }

            js.push_str("}\n");
            js
          }
        }

        #[ctor::ctor]
        unsafe fn #register() {
          use std::io::Write;
          WRITE_CALLBACKS.push(|file| write!(file, "{}", #self_name::to_js()))
        }
      }
    }
    Data::Enum(e) => {
      let mut getters: Vec<proc_macro2::TokenStream> = Vec::new();
      let mut setters: Vec<proc_macro2::TokenStream> = Vec::new();
      let mut variants = Vec::new();
      for variant in e.variants.iter() {
        let name = &variant.ident;
        variants.push(format!("'{}'", name.to_string().to_case(Case::Kebab)));
        getters.push(quote! {
          let name = stringify!(#name).to_case(Case::Kebab);
          js.push_str(&format!(
        r#"
      case {}:
        return '{}';"#,
            #self_name::#name as usize,
            name
          ));
        });
        setters.push(quote! {
          let name = stringify!(#name).to_case(Case::Kebab);
          js.push_str(&format!(
        r#"
      case '{}':
        write(addr, {});
        break;"#,
            name,
            #self_name::#name as usize
          ));
        });
      }

      let variants = variants.join(" | ");
      quote! {
        #[automatically_derived]
        impl ToJs for #self_name {
          fn to_js() -> String {
            use convert_case::{Case, Casing};
            let size = std::mem::size_of::<#self_name>();
            let heap = match size {
              1 => "U8",
              2 => "U16",
              4 => "U32",
              _ => todo!()
            };
            let mut js = String::new();
            js.push_str(&format!(
              r#"type {name}Variants = {variants};

export class {name} {{
  static get(addr: number): {name}Variants {{
    switch (read{heap}(addr)) {{"#,
              name = stringify!(#self_name),
              heap = heap,
              variants = #variants
            ));

            #(#getters);*;
            js.push_str(&format!(
              r#"
      default:
        throw new Error(`Unknown {name} value: ${{read{heap}(addr)}}`);
    }}
  }}

  static set(addr: number, value: {name}Variants): void {{
    let write = write{heap};
    switch (value) {{"#,
              name = stringify!(#self_name),
              heap = heap,
            ));

            #(#setters);*;
            js.push_str(&format!(
              r#"
      default:
        throw new Error(`Unknown {} value: ${{value}}`);
    }}
  }}
}}
"#,
              stringify!(#self_name)
            ));

            js
          }
        }

        #[ctor::ctor]
        unsafe fn #register() {
          use std::io::Write;
          WRITE_CALLBACKS.push(|file| write!(file, "{}", #self_name::to_js()))
        }
      }
    }
    _ => todo!(),
  };

  output.into()
}

#[proc_macro_derive(JsValue)]
pub fn derive_js_value(input: TokenStream) -> TokenStream {
  let DeriveInput {
    ident: self_name,
    data,
    ..
  } = parse_macro_input!(input);

  let output = match data {
    // Special handling for newtype structs.
    Data::Struct(s) if s.fields.len() == 1 && matches!(s.fields, Fields::Unnamed(_)) => {
      let ty = &s.fields.iter().next().unwrap().ty;
      quote! {
        #[automatically_derived]
        impl JsValue for #self_name {
          fn js_getter(addr: usize) -> String {
            <#ty>::js_getter(addr)
          }

          fn js_setter(addr: usize, value: &str) -> String {
            <#ty>::js_setter(addr, value)
          }

          fn ty() -> String {
            <#ty>::ty()
          }
        }
      }
    }
    _ => {
      let ty = match data {
        Data::Enum(_) => format!("{}Variants", self_name.to_string()),
        Data::Struct(_) => self_name.to_string(),
        _ => todo!(),
      };
      quote! {
        #[automatically_derived]
        impl JsValue for #self_name {
          fn js_getter(addr: usize) -> String {
            format!("{}.get(this.addr + {:?})", stringify!(#self_name), addr)
          }

          fn js_setter(addr: usize, value: &str) -> String {
            format!("{}.set(this.addr + {:?}, {})", stringify!(#self_name), addr, value)
          }

          fn ty() -> String {
            #ty.into()
          }
        }
      }
    }
  };

  output.into()
}

#[proc_macro_derive(SlabAllocated)]
pub fn derive_slab_allocated(input: TokenStream) -> TokenStream {
  let DeriveInput {
    ident: self_name,
    generics,
    ..
  } = parse_macro_input!(input);

  let slab_name = Ident::new(
    &format!("{}_SLAB", self_name.to_string().to_uppercase()),
    Span::call_site(),
  );

  let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

  let output = quote! {
    #[thread_local]
    static mut #slab_name: Slab<#self_name> = Slab::new();

    #[automatically_derived]
    impl #impl_generics SlabAllocated for #self_name #ty_generics #where_clause {
      fn alloc(count: u32) -> (u32, *mut #self_name) {
        unsafe {
          let addr = #slab_name.alloc(count);
          (addr, HEAP.get(addr))
        }
      }

      fn dealloc(addr: u32, count: u32) {
        unsafe {
          #slab_name.dealloc(addr, count);
        }
      }
    }
  };

  output.into()
}
