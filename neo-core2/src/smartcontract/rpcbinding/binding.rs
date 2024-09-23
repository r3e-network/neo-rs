use std::cmp;
use std::fmt;
use std::slice;
use std::string::String;
use std::collections::HashMap;
use std::convert::TryFrom;
use text_template::Template;
use unicode::UnicodeChar;

use crate::smartcontract;
use crate::smartcontract::binding;
use crate::smartcontract::manifest;
use crate::smartcontract::manifest::standard;
use crate::util;

// The set of constants containing parts of RPC binding template. Each block of code
// including template definition and var/type/method definitions contain new line at the
// start and ends with a new line. On adding new block of code to the template, please,
// ensure that this block has new line at the start and in the end of the block.
const EVENT_DEFINITION: &str = r#"
{{ define "EVENT" }}
/// {{.Name}} represents "{{.ManifestName}}" event emitted by the contract.
#[derive(Debug, Clone)]
pub struct {{.Name}} {
    {{- for $index, $arg in .Parameters}}
    pub {{ to_snake_case .Name}}: {{.Type}},
    {{- endfor}}
}
{{ end }}"#;

const SAFEMETHOD_DEFINITION: &str = r#"
{{ define "SAFEMETHOD" }}
/// {{.Name}} {{.Comment}}
pub fn {{to_snake_case .Name}}(&self{{range $index, $arg := .Arguments -}}
    {{- if ne $index 0}}, {{end}}
    {{- to_snake_case .Name}}: {{.Type}}
    {{- end}}) -> Result<{{if .ReturnType }}{{ .ReturnType }}{{else}}(){{end}}, Error> {
    {{if .ReturnType }}{{if and (not .ItemTo) (eq .Unwrapper "Item")}}let item = {{end}}{{if .ItemTo}}item_to_{{ .ItemTo }}({{end}}
    unwrap::{{.Unwrapper}}(self.invoker.call(&self.hash, "{{ .NameABI }}"
    {{- range $arg := .Arguments -}}, {{to_snake_case .Name}}{{end -}} )?){{if or .ItemTo (eq .Unwrapper "Item")}})?{{end}}{{else}}
    self.invoker.call(&self.hash, "{{ .NameABI }}"
    {{- range $arg := .Arguments -}}, {{to_snake_case .Name}}{{end}})?;
    Ok(()){{end}}
}
{{ if eq .Unwrapper "SessionIterator" }}
/// {{.Name}}_expanded is similar to {{to_snake_case .Name}} (uses the same contract
/// method), but can be useful if the server used doesn't support sessions and
/// doesn't expand iterators. It creates a script that will get the specified
/// number of result items from the iterator right in the VM and return them to
/// you. It's only limited by VM stack and GAS available for RPC invocations.
pub fn {{to_snake_case .Name}}_expanded(&self, {{range $index, $arg := .Arguments}}{{to_snake_case .Name}}: {{.Type}}, {{end}}num_of_iterator_items: i32) -> Result<Vec<stackitem::Item>, Error> {
    unwrap::array(self.invoker.call_and_expand_iterator(&self.hash, "{{.NameABI}}", num_of_iterator_items{{range $arg := .Arguments}}, {{to_snake_case .Name}}{{end}})?)
}
{{ end }}{{ end }}"#;

const METHOD_DEFINITION: &str = r#"
{{ define "METHOD" }}{{ if eq .ReturnType "bool"}}
fn script_for_{{to_snake_case .Name}}(&self{{range $index, $arg := .Arguments -}}
    {{- if ne $index 0}}, {{end}}
    {{- to_snake_case .Name}}: {{.Type}}
    {{- end}}) -> Result<Vec<u8>, Error> {
    smartcontract::create_call_with_assert_script(&self.hash, "{{ .NameABI }}"{{- range $index, $arg := .Arguments -}}, {{to_snake_case .Name}}{{end}})
}
{{ end }}
/// {{.Name}} {{.Comment}}
/// This transaction is signed and immediately sent to the network.
/// The values returned are its hash, ValidUntilBlock value and error if any.
pub fn {{to_snake_case .Name}}(&self{{range $index, $arg := .Arguments -}}
    {{- if ne $index 0}}, {{end}}
    {{- to_snake_case .Name}}: {{.Type}}
    {{- end}}) -> Result<(util::Uint256, u32), Error> {
    {{if ne .ReturnType "bool"}}self.actor.send_call(&self.hash, "{{ .NameABI }}"
    {{- range $index, $arg := .Arguments -}}, {{to_snake_case .Name}}{{end}}){{else}}let script = self.script_for_{{to_snake_case .Name}}({{- range $index, $arg := .Arguments -}}{{- if ne $index 0}}, {{end}}{{to_snake_case .Name}}{{end}})?;
    self.actor.send_run(&script){{end}}
}

/// {{.Name}}_transaction {{.Comment}}
/// This transaction is signed, but not sent to the network, instead it's
/// returned to the caller.
pub fn {{to_snake_case .Name}}_transaction(&self{{range $index, $arg := .Arguments -}}
    {{- if ne $index 0}}, {{end}}
    {{- to_snake_case .Name}}: {{.Type}}
    {{- end}}) -> Result<transaction::Transaction, Error> {
    {{if ne .ReturnType "bool"}}self.actor.make_call(&self.hash, "{{ .NameABI }}"
    {{- range $index, $arg := .Arguments -}}, {{to_snake_case .Name}}{{end}}){{else}}let script = self.script_for_{{to_snake_case .Name}}({{- range $index, $arg := .Arguments -}}{{- if ne $index 0}}, {{end}}{{to_snake_case .Name}}{{end}})?;
    self.actor.make_run(&script){{end}}
}

/// {{.Name}}_unsigned {{.Comment}}
/// This transaction is not signed, it's simply returned to the caller.
/// Any fields of it that do not affect fees can be changed (ValidUntilBlock,
/// Nonce), fee values (NetworkFee, SystemFee) can be increased as well.
pub fn {{to_snake_case .Name}}_unsigned(&self{{range $index, $arg := .Arguments -}}
    {{- if ne $index 0}}, {{end}}
    {{- to_snake_case .Name}}: {{.Type}}
    {{- end}}) -> Result<transaction::Transaction, Error> {
    {{if ne .ReturnType "bool"}}self.actor.make_unsigned_call(&self.hash, "{{ .NameABI }}", None
    {{- range $index, $arg := .Arguments -}}, {{to_snake_case .Name}}{{end}}){{else}}let script = self.script_for_{{to_snake_case .Name}}({{- range $index, $arg := .Arguments -}}{{- if ne $index 0}}, {{end}}{{to_snake_case .Name}}{{end}})?;
    self.actor.make_unsigned_run(&script, None){{end}}
}
{{end}}"#;

const BINDING_DEFINITION: &str = r#"// Code generated by neo-go contract generate-rpcwrapper --manifest <file.json> --out <file.rs> [--hash <hash>] [--config <config>]; DO NOT EDIT.

//! RPC wrappers for {{.ContractName}} contract.

{{range $m := .Imports}}use {{ $m }};
{{end}}
{{if len .Hash}}
/// Hash contains contract hash.
pub const HASH: util::Uint160 = {{ .Hash }};
{{end -}}
{{- range $index, $typ := .NamedTypes }}
/// {{to_pascal_case $typ.Name}} is a contract-specific {{$typ.Name}} type used by its methods.
#[derive(Debug, Clone)]
pub struct {{to_pascal_case $typ.Name}} {
{{- range $m := $typ.Fields}}
    pub {{ to_snake_case .Field}}: {{etTypeToStr .ExtendedType}},
{{- end}}
}
{{end}}
{{- range $e := .CustomEvents }}{{template "EVENT" $e }}{{ end -}}
{{- if .HasReader}}
/// Invoker is used by ContractReader to call various safe methods.
pub trait Invoker: {{if or .IsNep11D .IsNep11ND}}nep11::Invoker
{{else -}}
{{ if .IsNep17}}nep17::Invoker
{{else if len .SafeMethods}}Call
{{end -}}
{{if .HasIterator}} + CallAndExpandIterator + TerminateSession + TraverseIterator
{{end -}}
{{end -}} {}

/// Call is a trait for making contract calls.
pub trait Call {
    fn call(&self, contract: &util::Uint160, operation: &str, params: &[&dyn ToStackItem]) -> Result<result::Invoke, Error>;
}

/// CallAndExpandIterator is a trait for making contract calls and expanding iterators.
pub trait CallAndExpandIterator {
    fn call_and_expand_iterator(&self, contract: &util::Uint160, method: &str, max_items: i32, params: &[&dyn ToStackItem]) -> Result<result::Invoke, Error>;
}

/// TerminateSession is a trait for terminating sessions.
pub trait TerminateSession {
    fn terminate_session(&self, session_id: uuid::Uuid) -> Result<(), Error>;
}

/// TraverseIterator is a trait for traversing iterators.
pub trait TraverseIterator {
    fn traverse_iterator(&self, session_id: uuid::Uuid, iterator: &result::Iterator, num: i32) -> Result<Vec<stackitem::Item>, Error>;
}
{{end -}}
{{- if .HasWriter}}
/// Actor is used by Contract to call state-changing methods.
pub trait Actor: {{- if .HasReader}}
    Invoker +
{{end}}
{{- if or .IsNep11D .IsNep11ND}}
    nep11::Actor +
{{else if .IsNep17}}
    nep17::Actor +
{{end}}
{{- if len .Methods}}
    MakeCall + MakeRun + MakeUnsignedCall + MakeUnsignedRun + SendCall + SendRun
{{end -}} {}

/// MakeCall is a trait for making contract calls.
pub trait MakeCall {
    fn make_call(&self, contract: &util::Uint160, method: &str, params: &[&dyn ToStackItem]) -> Result<transaction::Transaction, Error>;
}

/// MakeRun is a trait for making contract runs.
pub trait MakeRun {
    fn make_run(&self, script: &[u8]) -> Result<transaction::Transaction, Error>;
}

/// MakeUnsignedCall is a trait for making unsigned contract calls.
pub trait MakeUnsignedCall {
    fn make_unsigned_call(&self, contract: &util::Uint160, method: &str, attrs: Option<&[transaction::Attribute]>, params: &[&dyn ToStackItem]) -> Result<transaction::Transaction, Error>;
}

/// MakeUnsignedRun is a trait for making unsigned contract runs.
pub trait MakeUnsignedRun {
    fn make_unsigned_run(&self, script: &[u8], attrs: Option<&[transaction::Attribute]>) -> Result<transaction::Transaction, Error>;
}

/// SendCall is a trait for sending contract calls.
pub trait SendCall {
    fn send_call(&self, contract: &util::Uint160, method: &str, params: &[&dyn ToStackItem]) -> Result<(util::Uint256, u32), Error>;
}

/// SendRun is a trait for sending contract runs.
pub trait SendRun {
    fn send_run(&self, script: &[u8]) -> Result<(util::Uint256, u32), Error>;
}
{{end -}}
{{- if .HasReader}}
/// ContractReader implements safe contract methods.
pub struct ContractReader<T: Invoker> {
    {{if .IsNep11D}}nep11_divisible_reader: nep11::DivisibleReader<T>,
    {{end -}}
    {{if .IsNep11ND}}nep11_non_divisible_reader: nep11::NonDivisibleReader<T>,
    {{end -}}
    {{if .IsNep17}}nep17_token_reader: nep17::TokenReader<T>,
    {{end -}}
    invoker: T,
    hash: util::Uint160,
}
{{end -}}
{{- if .HasWriter}}
/// Contract implements all contract methods.
pub struct Contract<T: Actor> {
    {{if .HasReader}}reader: ContractReader<T>,
    {{end -}}
    {{if .IsNep11D}}nep11_divisible_writer: nep11::DivisibleWriter<T>,
    {{end -}}
    {{if .IsNep11ND}}nep11_base_writer: nep11::BaseWriter<T>,
    {{end -}}
    {{if .IsNep17}}nep17_token_writer: nep17::TokenWriter<T>,
    {{end -}}
    actor: T,
    hash: util::Uint160,
}
{{end -}}
{{- if .HasReader}}
impl<T: Invoker> ContractReader<T> {
    /// Creates a new instance of ContractReader using {{if len .Hash -}}HASH{{- else -}}provided contract hash{{- end}} and the given Invoker.
    pub fn new(invoker: T{{- if not (len .Hash) -}}, hash: util::Uint160{{- end -}}) -> Self {
        {{if len .Hash -}}
        let hash = HASH;
        {{end -}}
        Self {
            {{- if .IsNep11D}}nep11_divisible_reader: nep11::DivisibleReader::new(invoker.clone(), hash), {{end}}
            {{- if .IsNep11ND}}nep11_non_divisible_reader: nep11::NonDivisibleReader::new(invoker.clone(), hash), {{end}}
            {{- if .IsNep17}}nep17_token_reader: nep17::TokenReader::new(invoker.clone(), hash), {{end -}}
            invoker,
            hash,
        }
    }
}
{{end -}}
{{- if .HasWriter}}
impl<T: Actor> Contract<T> {
    /// Creates a new instance of Contract using {{if len .Hash -}}HASH{{- else -}}provided contract hash{{- end}} and the given Actor.
    pub fn new(actor: T{{- if not (len .Hash) -}}, hash: util::Uint160{{- end -}}) -> Self {
        {{if len .Hash -}}
        let hash = HASH;
        {{end -}}
        {{if .IsNep11D}}let nep11dt = nep11::Divisible::new(actor.clone(), hash);
        {{end -}}
        {{if .IsNep11ND}}let nep11ndt = nep11::NonDivisible::new(actor.clone(), hash);
        {{end -}}
        {{if .IsNep17}}let nep17t = nep17::New::new(actor.clone(), hash);
        {{end -}}
        Self {
            {{- if .HasReader}}reader: ContractReader {
            {{- if .IsNep11D}}nep11_divisible_reader: nep11dt.reader(), {{end -}}
            {{- if .IsNep11ND}}nep11_non_divisible_reader: nep11ndt.reader(), {{end -}}
            {{- if .IsNep17}}nep17_token_reader: nep17t.reader(), {{end -}}
            invoker: actor.clone(),
            hash,
            }, {{end -}}
            {{- if .IsNep11D}}nep11_divisible_writer: nep11dt.writer(), {{end -}}
            {{- if .IsNep11ND}}nep11_base_writer: nep11ndt.writer(), {{end -}}
            {{- if .IsNep17}}nep17_token_writer: nep17t.writer(), {{end -}}
            actor,
            hash,
        }
    }
}
{{end -}}
{{- range $m := .SafeMethods }}{{template "SAFEMETHOD" $m }}{{ end -}}
{{- range $m := .Methods -}}{{template "METHOD" $m }}{{ end -}}
{{- range $index, $typ := .NamedTypes }}
/// Converts stack item into {{to_pascal_case $typ.Name}}.
/// NULL item is returned as None without error.
pub fn item_to_{{to_snake_case $typ.Name}}(item: stackitem::Item) -> Result<Option<{{to_pascal_case $typ.Name}}>, Error> {
    if item.type_() == stackitem::Type::Null {
        return Ok(None);
    }
    let mut res = {{to_pascal_case $typ.Name}} {
        {{- range $m := $typ.Fields}}
        {{ to_snake_case .Field}}: Default::default(),
        {{- end}}
    };
    res.from_stack_item(item)?;
    Ok(Some(res))
}

impl TryFrom<stackitem::Item> for {{to_pascal_case $typ.Name}} {
    type Error = Error;

    fn try_from(item: stackitem::Item) -> Result<Self, Self::Error> {
        let arr = item.as_array().ok_or_else(|| Error::new("not an array"))?;
        if arr.len() != {{len $typ.Fields}} {
            return Err(Error::new("wrong number of structure elements"));
        }
{{if len .Fields}}
        let mut res = Self {
            {{- range $m := $typ.Fields}}
            {{ to_snake_case .Field}}: Default::default(),
            {{- end}}
        };
        {{- range $index, $m := $typ.Fields}}
        res.{{ to_snake_case .Field}} = {{etTypeConverter .ExtendedType (printf "arr[%d]" $index)}}?;
        {{end}}
        Ok(res)
{{- else}}
        Ok(Self {})
{{- end}}
    }
}
{{ end -}}
{{- range $e := .CustomEvents }}
/// Retrieves a set of all emitted events with "{{.ManifestName}}" name from the provided ApplicationLog.
pub fn {{to_snake_case .Name}}s_from_application_log(log: &result::ApplicationLog) -> Result<Vec<{{.Name}}>, Error> {
    if log.is_none() {
        return Err(Error::new("nil application log"));
    }

    let mut res = Vec::new();
    for (i, ex) in log.executions.iter().enumerate() {
        for (j, e) in ex.events.iter().enumerate() {
            if e.name != "{{.ManifestName}}" {
                continue;
            }
            let event = {{.Name}}::try_from(e.item.clone())
                .map_err(|err| Error::new(&format!("failed to deserialize {{.Name}} from stackitem (execution #{}, event #{}): {}", i, j, err)))?;
            res.push(event);
        }
    }

    Ok(res)
}

impl TryFrom<stackitem::Item> for {{.Name}} {
    type Error = Error;

    fn try_from(item: stackitem::Item) -> Result<Self, Self::Error> {
        let arr = item.as_array().ok_or_else(|| Error::new("not an array"))?;
        if arr.len() != {{len .Parameters}} {
            return Err(Error::new("wrong number of structure elements"));
        }

        {{if len .Parameters}}Ok(Self {
            {{- range $index, $p := .Parameters}}
            {{ to_snake_case .Name}}: {{etTypeConverter .ExtType (printf "arr[%d]" $index)}}?,
            {{- end}}
        }){{else}}Ok(Self {}){{end}}
    }
}
{{end -}}"#;

const SRC_TMPL: &str = BINDING_DEFINITION.to_owned() + EVENT_DEFINITION + SAFEMETHOD_DEFINITION + METHOD_DEFINITION;

pub struct ContractTmpl {
    pub contract_tmpl: binding::ContractTmpl,

    pub safe_methods: Vec<SafeMethodTmpl>,
    pub custom_events: Vec<CustomEventTemplate>,
    pub named_types: Vec<binding::ExtendedType>,

    pub is_nep11d: bool,
    pub is_nep11nd: bool,
    pub is_nep17: bool,

    pub has_reader: bool,
    pub has_writer: bool,
    pub has_iterator: bool,
}

pub struct SafeMethodTmpl {
    pub method_tmpl: binding::MethodTmpl,
    pub unwrapper: String,
    pub item_to: String,
    pub extended_return: binding::ExtendedType,
}

pub struct CustomEventTemplate {
    // Name is the event's name that will be used as the event structure name in
    // the resulting RPC binding. It is a valid go structure name and may differ
    // from ManifestName.
    pub name: String,
    // ManifestName is the event's name declared in the contract manifest.
    // It may contain any UTF8 character.
    pub manifest_name: String,
    pub parameters: Vec<EventParamTmpl>,
}

pub struct EventParamTmpl {
    pub param_tmpl: binding::ParamTmpl,

    // ExtType holds the event parameter's type information provided by Manifest,
    // i.e. simple types only.
    pub ext_type: binding::ExtendedType,
}

/// Creates a new Config instance.
pub fn new_config() -> binding::Config {
    binding::new_config()
}

/// Generates Rust file containing smartcontract bindings and writes it to the `cfg.output`.
/// It doesn't check manifest from Config for validity, incorrect manifest can
/// lead to unexpected results.
pub fn generate(cfg: &mut binding::Config) -> Result<(), Error> {
    // Avoid changing *cfg.manifest.
    let mut mfst = cfg.manifest.clone();
    mfst.abi.methods = mfst.abi.methods.clone();
    cfg.manifest = mfst;

    let mut imports = HashMap::new();
    let mut ctr = ContractTmpl {
        contract_tmpl: Default::default(),
        safe_methods: Vec::new(),
        custom_events: Vec::new(),
        named_types: Vec::new(),
        is_nep11d: false,
        is_nep11nd: false,
        is_nep17: false,
        has_reader: false,
        has_writer: false,
        has_iterator: false,
    };

    // Strip standard methods from NEP-XX packages.
    for std in &cfg.manifest.supported_standards {
        if std == &manifest::NEP11_STANDARD_NAME {
            imports.insert("crate::rpcclient::nep11".to_string(), ());
            if standard::comply_abi(&cfg.manifest, &standard::NEP11_DIVISIBLE).is_ok() {
                mfst.abi.methods = drop_std_methods(&mfst.abi.methods, &standard::NEP11_DIVISIBLE);
                ctr.is_nep11d = true;
            } else if standard::comply_abi(&cfg.manifest, &standard::NEP11_NON_DIVISIBLE).is_ok() {
                mfst.abi.methods = drop_std_methods(&mfst.abi.methods, &standard::NEP11_NON_DIVISIBLE);
                ctr.is_nep11nd = true;
            }
            mfst.abi.events = drop_std_events(&mfst.abi.events, &standard::NEP11_BASE);
            break; // Can't be NEP-17 at the same time.
        }
        if std == &manifest::NEP17_STANDARD_NAME && standard::comply_abi(&cfg.manifest, &standard::NEP17).is_ok() {
            mfst.abi.methods = drop_std_methods(&mfst.abi.methods, &standard::NEP17);
            imports.insert("crate::rpcclient::nep17".to_string(), ());
            ctr.is_nep17 = true;
            mfst.abi.events = drop_std_events(&mfst.abi.events, &standard::NEP17);
            break; // Can't be NEP-11 at the same time.
        }
    }

    // OnNepXXPayment handlers normally can't be called directly.
    if standard::comply_abi(&cfg.manifest, &standard::NEP11_PAYABLE).is_ok() {
        mfst.abi.methods = drop_std_methods(&mfst.abi.methods, &standard::NEP11_PAYABLE);
    }
    if standard::comply_abi(&cfg.manifest, &standard::NEP17_PAYABLE).is_ok() {
        mfst.abi.methods = drop_std_methods(&mfst.abi.methods, &standard::NEP17_PAYABLE);
    }

    ctr.contract_tmpl = binding::template_from_manifest(cfg, sc_type_to_rust);
    ctr = sc_template_to_rpc(cfg, ctr, &mut imports, sc_type_to_rust);
    ctr.named_types = cfg.named_types.values().cloned().collect();
    ctr.named_types.sort_by(|a, b| a.name.cmp(&b.name));

    // Check resulting named types and events don't have duplicating field names.
    for t in &ctr.named_types {
        let mut f_dict = HashMap::new();
        for n in &t.fields {
            let name = to_snake_case(&n.field);
            if f_dict.contains_key(&name) {
                return Err(Error::new(&format!("named type `{}` has two fields with identical resulting binding name `{}`", t.name, name)));
            }
            f_dict.insert(name, ());
        }
    }
    for e in &ctr.custom_events {
        let mut f_dict = HashMap::new();
        for n in &e.parameters {
            let name = to_snake_case(&n.param_tmpl.name);
            if f_dict.contains_key(&name) {
                return Err(Error::new(&format!("event `{}` has two fields with identical resulting binding name `{}`", e.name, name)));
            }
            f_dict.insert(name, ());
        }
    }

    let src_template = Template::new(SRC_TMPL)?;

    src_template.render_to_write(&ctr, &mut cfg.output)
        .map_err(|e| Error::new(&format!("failed to render template: {}", e)))
}

fn drop_manifest_methods(meths: &[manifest::Method], manifested: &[manifest::Method]) -> Vec<manifest::Method> {
    meths.iter().filter(|m| {
        !manifested.iter().any(|e| {
            m.name == e.name && m.parameters.len() == e.parameters.len()
        })
    }).cloned().collect()
}

fn drop_manifest_events(events: &[manifest::Event], manifested: &[manifest::Event]) -> Vec<manifest::Event> {
    events.iter().filter(|e| {
        !manifested.iter().any(|v| {
            e.name == v.name && e.parameters.len() == v.parameters.len()
        })
    }).cloned().collect()
}

fn drop_std_methods(meths: &[manifest::Method], std: &standard::Standard) -> Vec<manifest::Method> {
    let mut meths = drop_manifest_methods(meths, &std.manifest.abi.methods);
    if let Some(optional) = &std.optional {
        meths = drop_manifest_methods(&meths, optional);
    }
    if let Some(base) = &std.base {
        meths = drop_std_methods(&meths, base);
    }
    meths
}

fn drop_std_events(events: &[manifest::Event], std: &standard::Standard) -> Vec<manifest::Event> {
    let mut events = drop_manifest_events(events, &std.manifest.abi.events);
    if let Some(base) = &std.base {
        events = drop_std_events(&events, base);
    }
    events
}

fn extended_type_to_rust(et: &binding::ExtendedType, named: &HashMap<String, binding::ExtendedType>) -> (String, String) {
    match et.base {
        smartcontract::ParamType::Any => ("Box<dyn Any>".to_string(), "std::any".to_string()),
        smartcontract::ParamType::Bool => ("bool".to_string(), String::new()),
        smartcontract::ParamType::Integer => ("num_bigint::BigInt".to_string(), "num_bigint".to_string()),
        smartcontract::ParamType::ByteArray => ("Vec<u8>".to_string(), String::new()),
        smartcontract::ParamType::String => ("String".to_string(), String::new()),
        smartcontract::ParamType::Hash160 => ("util::Uint160".to_string(), "crate::util".to_string()),
        smartcontract::ParamType::Hash256 => ("util::Uint256".to_string(), "crate::util".to_string()),
        smartcontract::ParamType::PublicKey => ("keys::PublicKey".to_string(), "crate::crypto::keys".to_string()),
        smartcontract::ParamType::Signature => ("Vec<u8>".to_string(), String::new()),
        smartcontract::ParamType::Array => {
            if !et.name.is_empty() {
                (format!("Box<{}>", to_pascal_case(&et.name)), "crate::vm::stackitem".to_string())
            } else if let Some(value) = &et.value {
                if value.base == smartcontract::ParamType::PublicKey { // Special array wrapper.
                    ("keys::PublicKeys".to_string(), "crate::crypto::keys".to_string())
                } else {
                    let (sub, pkg) = extended_type_to_rust(value, named);
                    (format!("Vec<{}>", sub), pkg)
                }
            } else {
                ("Vec<Box<dyn Any>>".to_string(), String::new())
            }
        }
        smartcontract::ParamType::Map => {
            let (kt, _) = extended_type_to_rust(&binding::ExtendedType { base: et.key, ..Default::default() }, named);
            let vt = if let Some(value) = &et.value {
                let (vt, _) = extended_type_to_rust(value, named);
                vt
            } else {
import (
	"cmp"
	"fmt"
	"slices"
	"strings"
	"text/template"
	"unicode"

	"github.com/nspcc-dev/neo-go/pkg/smartcontract"
	"github.com/nspcc-dev/neo-go/pkg/smartcontract/binding"
	"github.com/nspcc-dev/neo-go/pkg/smartcontract/manifest"
	"github.com/nspcc-dev/neo-go/pkg/smartcontract/manifest/standard"
	"github.com/nspcc-dev/neo-go/pkg/util"
)

// The set of constants containing parts of RPC binding template. Each block of code
// including template definition and var/type/method definitions contain new line at the
// start and ends with a new line. On adding new block of code to the template, please,
// ensure that this block has new line at the start and in the end of the block.
const (
	eventDefinition = `{{ define "EVENT" }}
// {{.Name}} represents "{{.ManifestName}}" event emitted by the contract.
type {{.Name}} struct {
	{{- range $index, $arg := .Parameters}}
	{{ upperFirst .Name}} {{.Type}}
	{{- end}}
}
{{ end }}`

	safemethodDefinition = `{{ define "SAFEMETHOD" }}
// {{.Name}} {{.Comment}}
func (c *ContractReader) {{.Name}}({{range $index, $arg := .Arguments -}}
	{{- if ne $index 0}}, {{end}}
		{{- .Name}} {{.Type}}
	{{- end}}) {{if .ReturnType }}({{ .ReturnType }}, error) {
	return {{if and (not .ItemTo) (eq .Unwrapper "Item")}}func(item stackitem.Item, err error) ({{ .ReturnType }}, error) {
		if err != nil {
			return nil, err
		}
		return {{addIndent (etTypeConverter .ExtendedReturn "item") "\t"}}
	} ( {{- end -}} {{if .ItemTo -}} itemTo{{ .ItemTo }}( {{- end -}}
			unwrap.{{.Unwrapper}}(c.invoker.Call(c.hash, "{{ .NameABI }}"
		{{- range $arg := .Arguments -}}, {{.Name}}{{end -}} )) {{- if or .ItemTo (eq .Unwrapper "Item") -}} ) {{- end}}
	{{- else -}} (*result.Invoke, error) {
	c.invoker.Call(c.hash, "{{ .NameABI }}"
		{{- range $arg := .Arguments -}}, {{.Name}}{{end}})
	{{- end}}
}
{{ if eq .Unwrapper "SessionIterator" }}
// {{.Name}}Expanded is similar to {{.Name}} (uses the same contract
// method), but can be useful if the server used doesn't support sessions and
// doesn't expand iterators. It creates a script that will get the specified
// number of result items from the iterator right in the VM and return them to
// you. It's only limited by VM stack and GAS available for RPC invocations.
func (c *ContractReader) {{.Name}}Expanded({{range $index, $arg := .Arguments}}{{.Name}} {{.Type}}, {{end}}_numOfIteratorItems int) ([]stackitem.Item, error) {
	return unwrap.Array(c.invoker.CallAndExpandIterator(c.hash, "{{.NameABI}}", _numOfIteratorItems{{range $arg := .Arguments}}, {{.Name}}{{end}}))
}
{{ end }}{{ end }}`
	methodDefinition = `{{ define "METHOD" }}{{ if eq .ReturnType "bool"}}
func (c *Contract) scriptFor{{.Name}}({{range $index, $arg := .Arguments -}}
	{{- if ne $index 0}}, {{end}}
		{{- .Name}} {{.Type}}
	{{- end}}) ([]byte, error) {
	return smartcontract.CreateCallWithAssertScript(c.hash, "{{ .NameABI }}"{{- range $index, $arg := .Arguments -}}, {{.Name}}{{end}})
}
{{ end }}
// {{.Name}} {{.Comment}}
// This transaction is signed and immediately sent to the network.
// The values returned are its hash, ValidUntilBlock value and error if any.
func (c *Contract) {{.Name}}({{range $index, $arg := .Arguments -}}
	{{- if ne $index 0}}, {{end}}
		{{- .Name}} {{.Type}}
	{{- end}}) (util.Uint256, uint32, error) {
	{{if ne .ReturnType "bool"}}return c.actor.SendCall(c.hash, "{{ .NameABI }}"
	{{- range $index, $arg := .Arguments -}}, {{.Name}}{{end}}){{else}}script, err := c.scriptFor{{.Name}}({{- range $index, $arg := .Arguments -}}{{- if ne $index 0}}, {{end}}{{.Name}}{{end}})
	if err != nil {
		return util.Uint256{}, 0, err
	}
	return c.actor.SendRun(script){{end}}
}

// {{.Name}}Transaction {{.Comment}}
// This transaction is signed, but not sent to the network, instead it's
// returned to the caller.
func (c *Contract) {{.Name}}Transaction({{range $index, $arg := .Arguments -}}
	{{- if ne $index 0}}, {{end}}
		{{- .Name}} {{.Type}}
	{{- end}}) (*transaction.Transaction, error) {
	{{if ne .ReturnType "bool"}}return c.actor.MakeCall(c.hash, "{{ .NameABI }}"
	{{- range $index, $arg := .Arguments -}}, {{.Name}}{{end}}){{else}}script, err := c.scriptFor{{.Name}}({{- range $index, $arg := .Arguments -}}{{- if ne $index 0}}, {{end}}{{.Name}}{{end}})
	if err != nil {
		return nil, err
	}
	return c.actor.MakeRun(script){{end}}
}

// {{.Name}}Unsigned {{.Comment}}
// This transaction is not signed, it's simply returned to the caller.
// Any fields of it that do not affect fees can be changed (ValidUntilBlock,
// Nonce), fee values (NetworkFee, SystemFee) can be increased as well.
func (c *Contract) {{.Name}}Unsigned({{range $index, $arg := .Arguments -}}
	{{- if ne $index 0}}, {{end}}
		{{- .Name}} {{.Type}}
	{{- end}}) (*transaction.Transaction, error) {
	{{if ne .ReturnType "bool"}}return c.actor.MakeUnsignedCall(c.hash, "{{ .NameABI }}", nil
	{{- range $index, $arg := .Arguments -}}, {{.Name}}{{end}}){{else}}script, err := c.scriptFor{{.Name}}({{- range $index, $arg := .Arguments -}}{{- if ne $index 0}}, {{end}}{{.Name}}{{end}})
	if err != nil {
		return nil, err
	}
	return c.actor.MakeUnsignedRun(script, nil){{end}}
}
{{end}}`

	bindingDefinition = `// Code generated by neo-go contract generate-rpcwrapper --manifest <file.json> --out <file.go> [--hash <hash>] [--config <config>]; DO NOT EDIT.

// Package {{.PackageName}} contains RPC wrappers for {{.ContractName}} contract.
package {{.PackageName}}

import (
{{range $m := .Imports}}	"{{ $m }}"
{{end}})
{{if len .Hash}}
// Hash contains contract hash.
var Hash = {{ .Hash }}
{{end -}}
{{- range $index, $typ := .NamedTypes }}
// {{toTypeName $typ.Name}} is a contract-specific {{$typ.Name}} type used by its methods.
type {{toTypeName $typ.Name}} struct {
{{- range $m := $typ.Fields}}
	{{ upperFirst .Field}} {{etTypeToStr .ExtendedType}}
{{- end}}
}
{{end}}
{{- range $e := .CustomEvents }}{{template "EVENT" $e }}{{ end -}}
{{- if .HasReader}}
// Invoker is used by ContractReader to call various safe methods.
type Invoker interface {
{{if or .IsNep11D .IsNep11ND}}	nep11.Invoker
{{else -}}
{{ if .IsNep17}}	nep17.Invoker
{{else if len .SafeMethods}}	Call(contract util.Uint160, operation string, params ...any) (*result.Invoke, error)
{{end -}}
{{if .HasIterator}}	CallAndExpandIterator(contract util.Uint160, method string, maxItems int, params ...any) (*result.Invoke, error)
	TerminateSession(sessionID uuid.UUID) error
	TraverseIterator(sessionID uuid.UUID, iterator *result.Iterator, num int) ([]stackitem.Item, error)
{{end -}}
{{end -}}
}
{{end -}}
{{- if .HasWriter}}
// Actor is used by Contract to call state-changing methods.
type Actor interface {
{{- if .HasReader}}
	Invoker
{{end}}
{{- if or .IsNep11D .IsNep11ND}}
	nep11.Actor
{{else if .IsNep17}}
	nep17.Actor
{{end}}
{{- if len .Methods}}
	MakeCall(contract util.Uint160, method string, params ...any) (*transaction.Transaction, error)
	MakeRun(script []byte) (*transaction.Transaction, error)
	MakeUnsignedCall(contract util.Uint160, method string, attrs []transaction.Attribute, params ...any) (*transaction.Transaction, error)
	MakeUnsignedRun(script []byte, attrs []transaction.Attribute) (*transaction.Transaction, error)
	SendCall(contract util.Uint160, method string, params ...any) (util.Uint256, uint32, error)
	SendRun(script []byte) (util.Uint256, uint32, error)
{{end -}}
}
{{end -}}
{{- if .HasReader}}
// ContractReader implements safe contract methods.
type ContractReader struct {
	{{if .IsNep11D}}nep11.DivisibleReader
	{{end -}}
	{{if .IsNep11ND}}nep11.NonDivisibleReader
	{{end -}}
	{{if .IsNep17}}nep17.TokenReader
	{{end -}}
	invoker Invoker
	hash util.Uint160
}
{{end -}}
{{- if .HasWriter}}
// Contract implements all contract methods.
type Contract struct {
	{{if .HasReader}}ContractReader
	{{end -}}
	{{if .IsNep11D}}nep11.DivisibleWriter
	{{end -}}
	{{if .IsNep11ND}}nep11.BaseWriter
	{{end -}}
	{{if .IsNep17}}nep17.TokenWriter
	{{end -}}
	actor Actor
	hash util.Uint160
}
{{end -}}
{{- if .HasReader}}
// NewReader creates an instance of ContractReader using {{if len .Hash -}}Hash{{- else -}}provided contract hash{{- end}} and the given Invoker.
func NewReader(invoker Invoker{{- if not (len .Hash) -}}, hash util.Uint160{{- end -}}) *ContractReader {
	{{if len .Hash -}}
	var hash = Hash
	{{end -}}
	return &ContractReader{
		{{- if .IsNep11D}}*nep11.NewDivisibleReader(invoker, hash), {{end}}
		{{- if .IsNep11ND}}*nep11.NewNonDivisibleReader(invoker, hash), {{end}}
		{{- if .IsNep17}}*nep17.NewReader(invoker, hash), {{end -}}
		invoker, hash}
}
{{end -}}
{{- if .HasWriter}}
// New creates an instance of Contract using {{if len .Hash -}}Hash{{- else -}}provided contract hash{{- end}} and the given Actor.
func New(actor Actor{{- if not (len .Hash) -}}, hash util.Uint160{{- end -}}) *Contract {
	{{if len .Hash -}}
	var hash = Hash
	{{end -}}
	{{if .IsNep11D}}var nep11dt = nep11.NewDivisible(actor, hash)
	{{end -}}
	{{if .IsNep11ND}}var nep11ndt = nep11.NewNonDivisible(actor, hash)
	{{end -}}
	{{if .IsNep17}}var nep17t = nep17.New(actor, hash)
	{{end -}}
	return &Contract{
		{{- if .HasReader}}ContractReader{
		{{- if .IsNep11D}}nep11dt.DivisibleReader, {{end -}}
		{{- if .IsNep11ND}}nep11ndt.NonDivisibleReader, {{end -}}
		{{- if .IsNep17}}nep17t.TokenReader, {{end -}}
		actor, hash}, {{end -}}
		{{- if .IsNep11D}}nep11dt.DivisibleWriter, {{end -}}
		{{- if .IsNep11ND}}nep11ndt.BaseWriter, {{end -}}
		{{- if .IsNep17}}nep17t.TokenWriter, {{end -}}
		actor, hash}
}
{{end -}}
{{- range $m := .SafeMethods }}{{template "SAFEMETHOD" $m }}{{ end -}}
{{- range $m := .Methods -}}{{template "METHOD" $m }}{{ end -}}
{{- range $index, $typ := .NamedTypes }}
// itemTo{{toTypeName $typ.Name}} converts stack item into *{{toTypeName $typ.Name}}.
// NULL item is returned as nil pointer without error.
func itemTo{{toTypeName $typ.Name}}(item stackitem.Item, err error) (*{{toTypeName $typ.Name}}, error) {
	if err != nil {
		return nil, err
	}
	_, null := item.(stackitem.Null)
	if null {
		return nil, nil
	}
	var res = new({{toTypeName $typ.Name}})
	err = res.FromStackItem(item)
	return res, err
}

// FromStackItem retrieves fields of {{toTypeName $typ.Name}} from the given
// [stackitem.Item] or returns an error if it's not possible to do to so.
func (res *{{toTypeName $typ.Name}}) FromStackItem(item stackitem.Item) error {
	arr, ok := item.Value().([]stackitem.Item)
	if !ok {
		return errors.New("not an array")
	}
	if len(arr) != {{len $typ.Fields}} {
		return errors.New("wrong number of structure elements")
	}
{{if len .Fields}}
	var (
		index = -1
		err   error
	)
{{- range $m := $typ.Fields}}
	index++
	res.{{ upperFirst .Field}}, err = {{etTypeConverter .ExtendedType "arr[index]"}}
	if err != nil {
		return fmt.Errorf("field {{ upperFirst .Field}}: %w", err)
	}
{{end}}
{{- end}}
	return nil
}
{{ end -}}
{{- range $e := .CustomEvents }}
// {{$e.Name}}sFromApplicationLog retrieves a set of all emitted events
// with "{{$e.ManifestName}}" name from the provided [result.ApplicationLog].
func {{$e.Name}}sFromApplicationLog(log *result.ApplicationLog) ([]*{{$e.Name}}, error) {
	if log == nil {
		return nil, errors.New("nil application log")
	}

	var res []*{{$e.Name}}
	for i, ex := range log.Executions {
		for j, e := range ex.Events {
			if e.Name != "{{$e.ManifestName}}" {
				continue
			}
			event := new({{$e.Name}})
			err := event.FromStackItem(e.Item)
			if err != nil {
				return nil, fmt.Errorf("failed to deserialize {{$e.Name}} from stackitem (execution #%d, event #%d): %w", i, j, err)
			}
			res = append(res, event)
		}
	}

	return res, nil
}

// FromStackItem converts provided [stackitem.Array] to {{$e.Name}} or
// returns an error if it's not possible to do to so.
func (e *{{$e.Name}}) FromStackItem(item *stackitem.Array) error {
	if item == nil {
		return errors.New("nil item")
	}
	arr, ok := item.Value().([]stackitem.Item)
	if !ok {
		return errors.New("not an array")
	}
	if len(arr) != {{len $e.Parameters}} {
		return errors.New("wrong number of structure elements")
	}

	{{if len $e.Parameters}}var (
		index = -1
		err   error
	)
	{{- range $p := $e.Parameters}}
	index++
	e.{{ upperFirst .Name}}, err = {{etTypeConverter .ExtType "arr[index]"}}
	if err != nil {
		return fmt.Errorf("field {{ upperFirst .Name}}: %w", err)
	}
{{end}}
{{- end}}
	return nil
}
{{end -}}`

	srcTmpl = bindingDefinition +
		eventDefinition +
		safemethodDefinition +
		methodDefinition
)

type (
	ContractTmpl struct {
		binding.ContractTmpl

		SafeMethods  []SafeMethodTmpl
		CustomEvents []CustomEventTemplate
		NamedTypes   []binding.ExtendedType

		IsNep11D  bool
		IsNep11ND bool
		IsNep17   bool

		HasReader   bool
		HasWriter   bool
		HasIterator bool
	}

	SafeMethodTmpl struct {
		binding.MethodTmpl
		Unwrapper      string
		ItemTo         string
		ExtendedReturn binding.ExtendedType
	}

	CustomEventTemplate struct {
		// Name is the event's name that will be used as the event structure name in
		// the resulting RPC binding. It is a valid go structure name and may differ
		// from ManifestName.
		Name string
		// ManifestName is the event's name declared in the contract manifest.
		// It may contain any UTF8 character.
		ManifestName string
		Parameters   []EventParamTmpl
	}

	EventParamTmpl struct {
		binding.ParamTmpl

		// ExtType holds the event parameter's type information provided by Manifest,
		// i.e. simple types only.
		ExtType binding.ExtendedType
	}
)

// NewConfig initializes and returns a new config instance.
func NewConfig() binding.Config {
	return binding.NewConfig()
}

// Generate writes Go file containing smartcontract bindings to the `cfg.Output`.
// It doesn't check manifest from Config for validity, incorrect manifest can
// lead to unexpected results.
func Generate(cfg binding.Config) error {
	// Avoid changing *cfg.Manifest.
	mfst := *cfg.Manifest
	mfst.ABI.Methods = slices.Clone(mfst.ABI.Methods)
	cfg.Manifest = &mfst

	var imports = make(map[string]struct{})
	var ctr ContractTmpl

	// Strip standard methods from NEP-XX packages.
	for _, std := range cfg.Manifest.SupportedStandards {
		if std == manifest.NEP11StandardName {
			imports["github.com/nspcc-dev/neo-go/pkg/rpcclient/nep11"] = struct{}{}
			if standard.ComplyABI(cfg.Manifest, standard.Nep11Divisible) == nil {
				mfst.ABI.Methods = dropStdMethods(mfst.ABI.Methods, standard.Nep11Divisible)
				ctr.IsNep11D = true
			} else if standard.ComplyABI(cfg.Manifest, standard.Nep11NonDivisible) == nil {
				mfst.ABI.Methods = dropStdMethods(mfst.ABI.Methods, standard.Nep11NonDivisible)
				ctr.IsNep11ND = true
			}
			mfst.ABI.Events = dropStdEvents(mfst.ABI.Events, standard.Nep11Base)
			break // Can't be NEP-17 at the same time.
		}
		if std == manifest.NEP17StandardName && standard.ComplyABI(cfg.Manifest, standard.Nep17) == nil {
			mfst.ABI.Methods = dropStdMethods(mfst.ABI.Methods, standard.Nep17)
			imports["github.com/nspcc-dev/neo-go/pkg/rpcclient/nep17"] = struct{}{}
			ctr.IsNep17 = true
			mfst.ABI.Events = dropStdEvents(mfst.ABI.Events, standard.Nep17)
			break // Can't be NEP-11 at the same time.
		}
	}

	// OnNepXXPayment handlers normally can't be called directly.
	if standard.ComplyABI(cfg.Manifest, standard.Nep11Payable) == nil {
		mfst.ABI.Methods = dropStdMethods(mfst.ABI.Methods, standard.Nep11Payable)
	}
	if standard.ComplyABI(cfg.Manifest, standard.Nep17Payable) == nil {
		mfst.ABI.Methods = dropStdMethods(mfst.ABI.Methods, standard.Nep17Payable)
	}

	ctr.ContractTmpl = binding.TemplateFromManifest(cfg, scTypeToGo)
	ctr = scTemplateToRPC(cfg, ctr, imports, scTypeToGo)
	ctr.NamedTypes = make([]binding.ExtendedType, 0, len(cfg.NamedTypes))
	for k := range cfg.NamedTypes {
		ctr.NamedTypes = append(ctr.NamedTypes, cfg.NamedTypes[k])
	}
	slices.SortFunc(ctr.NamedTypes, func(a, b binding.ExtendedType) int { return strings.Compare(a.Name, b.Name) })

	// Check resulting named types and events don't have duplicating field names.
	for _, t := range ctr.NamedTypes {
		fDict := make(map[string]struct{})
		for _, n := range t.Fields {
			name := upperFirst(n.Field)
			if _, ok := fDict[name]; ok {
				return fmt.Errorf("named type `%s` has two fields with identical resulting binding name `%s`", t.Name, name)
			}
			fDict[name] = struct{}{}
		}
	}
	for _, e := range ctr.CustomEvents {
		fDict := make(map[string]struct{})
		for _, n := range e.Parameters {
			name := upperFirst(n.Name)
			if _, ok := fDict[name]; ok {
				return fmt.Errorf("event `%s` has two fields with identical resulting binding name `%s`", e.Name, name)
			}
			fDict[name] = struct{}{}
		}
	}

	var srcTemplate = template.Must(template.New("generate").Funcs(template.FuncMap{
		"addIndent":       addIndent,
		"etTypeConverter": etTypeConverter,
		"etTypeToStr": func(et binding.ExtendedType) string {
			r, _ := extendedTypeToGo(et, cfg.NamedTypes)
			return r
		},
		"toTypeName": toTypeName,
		"cutPointer": cutPointer,
		"upperFirst": upperFirst,
	}).Parse(srcTmpl))

	return binding.FExecute(srcTemplate, cfg.Output, ctr)
}

func dropManifestMethods(meths []manifest.Method, manifested []manifest.Method) []manifest.Method {
	return slices.DeleteFunc(meths, func(m manifest.Method) bool {
		return slices.ContainsFunc(manifested, func(e manifest.Method) bool {
			return 0 == cmp.Or(
				cmp.Compare(m.Name, e.Name),
				cmp.Compare(len(m.Parameters), len(e.Parameters)),
			)
		})
	})
}

func dropManifestEvents(events []manifest.Event, manifested []manifest.Event) []manifest.Event {
	return slices.DeleteFunc(events, func(e manifest.Event) bool {
		return slices.ContainsFunc(manifested, func(v manifest.Event) bool {
			return 0 == cmp.Or(
				cmp.Compare(e.Name, v.Name),
				cmp.Compare(len(e.Parameters), len(v.Parameters)),
			)
		})
	})
}

func dropStdMethods(meths []manifest.Method, std *standard.Standard) []manifest.Method {
	meths = dropManifestMethods(meths, std.Manifest.ABI.Methods)
	if std.Optional != nil {
		meths = dropManifestMethods(meths, std.Optional)
	}
	if std.Base != nil {
		return dropStdMethods(meths, std.Base)
	}
	return meths
}

func dropStdEvents(events []manifest.Event, std *standard.Standard) []manifest.Event {
	events = dropManifestEvents(events, std.Manifest.ABI.Events)
	if std.Base != nil {
		return dropStdEvents(events, std.Base)
	}
	return events
}

func extendedTypeToGo(et binding.ExtendedType, named map[string]binding.ExtendedType) (string, string) {
	switch et.Base {
	case smartcontract.AnyType:
		return "any", ""
	case smartcontract.BoolType:
		return "bool", ""
	case smartcontract.IntegerType:
		return "*big.Int", "math/big"
	case smartcontract.ByteArrayType:
		return "[]byte", ""
	case smartcontract.StringType:
		return "string", ""
	case smartcontract.Hash160Type:
		return "util.Uint160", "github.com/nspcc-dev/neo-go/pkg/util"
	case smartcontract.Hash256Type:
		return "util.Uint256", "github.com/nspcc-dev/neo-go/pkg/util"
	case smartcontract.PublicKeyType:
		return "*keys.PublicKey", "github.com/nspcc-dev/neo-go/pkg/crypto/keys"
	case smartcontract.SignatureType:
		return "[]byte", ""
	case smartcontract.ArrayType:
		if len(et.Name) > 0 {
			return "*" + toTypeName(et.Name), "github.com/nspcc-dev/neo-go/pkg/vm/stackitem"
		} else if et.Value != nil {
			if et.Value.Base == smartcontract.PublicKeyType { // Special array wrapper.
				return "keys.PublicKeys", "github.com/nspcc-dev/neo-go/pkg/crypto/keys"
			}
			sub, pkg := extendedTypeToGo(*et.Value, named)
			return "[]" + sub, pkg
		}
		return "[]any", ""

	case smartcontract.MapType:
		kt, _ := extendedTypeToGo(binding.ExtendedType{Base: et.Key}, named)
		var vt string
		if et.Value != nil {
			vt, _ = extendedTypeToGo(*et.Value, named)
		} else {
			vt = "any"
		}
		return "map[" + kt + "]" + vt, "github.com/nspcc-dev/neo-go/pkg/vm/stackitem"
	case smartcontract.InteropInterfaceType:
		return "any", ""
	case smartcontract.VoidType:
		return "", ""
	}
	panic("unreachable")
}

func etTypeConverter(et binding.ExtendedType, v string) string {
	switch et.Base {
	case smartcontract.AnyType:
		return v + ".Value(), error(nil)"
	case smartcontract.BoolType:
		return v + ".TryBool()"
	case smartcontract.IntegerType:
		return v + ".TryInteger()"
	case smartcontract.ByteArrayType, smartcontract.SignatureType:
		return v + ".TryBytes()"
	case smartcontract.StringType:
		return `func(item stackitem.Item) (string, error) {
		b, err := item.TryBytes()
		if err != nil {
			return "", err
		}
		if !utf8.Valid(b) {
			return "", errors.New("not a UTF-8 string")
		}
		return string(b), nil
	}(` + v + `)`
	case smartcontract.Hash160Type:
		return `func(item stackitem.Item) (util.Uint160, error) {
		b, err := item.TryBytes()
		if err != nil {
			return util.Uint160{}, err
		}
		u, err := util.Uint160DecodeBytesBE(b)
		if err != nil {
			return util.Uint160{}, err
		}
		return u, nil
	}(` + v + `)`
	case smartcontract.Hash256Type:
		return `func(item stackitem.Item) (util.Uint256, error) {
		b, err := item.TryBytes()
		if err != nil {
			return util.Uint256{}, err
		}
		u, err := util.Uint256DecodeBytesBE(b)
		if err != nil {
			return util.Uint256{}, err
		}
		return u, nil
	}(` + v + `)`
	case smartcontract.PublicKeyType:
		return `func(item stackitem.Item) (*keys.PublicKey, error) {
		b, err := item.TryBytes()
		if err != nil {
			return nil, err
		}
		k, err := keys.NewPublicKeyFromBytes(b, elliptic.P256())
		if err != nil {
			return nil, err
		}
		return k, nil
	}(` + v + `)`
	case smartcontract.ArrayType:
		if len(et.Name) > 0 {
			return "itemTo" + toTypeName(et.Name) + "(" + v + ", nil)"
		} else if et.Value != nil {
			at, _ := extendedTypeToGo(et, nil)
			return `func(item stackitem.Item) (` + at + `, error) {
		arr, ok := item.Value().([]stackitem.Item)
		if !ok {
			return nil, errors.New("not an array")
		}
		res := make(` + at + `, len(arr))
		for i := range res {
			res[i], err = ` + addIndent(etTypeConverter(*et.Value, "arr[i]"), "\t\t") + `
			if err != nil {
				return nil, fmt.Errorf("item %d: %w", i, err)
			}
		}
		return res, nil
	}(` + v + `)`
		}
		return etTypeConverter(binding.ExtendedType{
			Base: smartcontract.ArrayType,
			Value: &binding.ExtendedType{
				Base: smartcontract.AnyType,
			},
		}, v)

	case smartcontract.MapType:
		if et.Value != nil {
			at, _ := extendedTypeToGo(et, nil)
			return `func(item stackitem.Item) (` + at + `, error) {
		m, ok := item.Value().([]stackitem.MapElement)
		if !ok {
			return nil, fmt.Errorf("%s is not a map", item.Type().String())
		}
		res := make(` + at + `)
		for i := range m {
			k, err := ` + addIndent(etTypeConverter(binding.ExtendedType{Base: et.Key}, "m[i].Key"), "\t\t") + `
			if err != nil {
				return nil, fmt.Errorf("key %d: %w", i, err)
			}
			v, err := ` + addIndent(etTypeConverter(*et.Value, "m[i].Value"), "\t\t") + `
			if err != nil {
				return nil, fmt.Errorf("value %d: %w", i, err)
			}
			res[k] = v
		}
		return res, nil
	}(` + v + `)`
		}
		return etTypeConverter(binding.ExtendedType{
			Base: smartcontract.MapType,
			Key:  et.Key,
			Value: &binding.ExtendedType{
				Base: smartcontract.AnyType,
			},
		}, v)
	case smartcontract.InteropInterfaceType:
		return "item.Value(), nil"
	case smartcontract.VoidType:
		return ""
	}
	panic("unreachable")
}

func scTypeToGo(name string, typ smartcontract.ParamType, cfg *binding.Config) (string, string) {
	et, ok := cfg.Types[name]
	if !ok {
		et = binding.ExtendedType{Base: typ}
	}
	return extendedTypeToGo(et, cfg.NamedTypes)
}

func scTemplateToRPC(cfg binding.Config, ctr ContractTmpl, imports map[string]struct{}, scTypeConverter func(string, smartcontract.ParamType, *binding.Config) (string, string)) ContractTmpl {
	for i := range ctr.Imports {
		imports[ctr.Imports[i]] = struct{}{}
	}
	if !cfg.Hash.Equals(util.Uint160{}) {
		ctr.Hash = fmt.Sprintf("%#v", cfg.Hash)
	}
	for i := 0; i < len(ctr.Methods); i++ {
		abim := cfg.Manifest.ABI.GetMethod(ctr.Methods[i].NameABI, len(ctr.Methods[i].Arguments))
		if abim.Safe {
			ctr.SafeMethods = append(ctr.SafeMethods, SafeMethodTmpl{MethodTmpl: ctr.Methods[i]})
			et, ok := cfg.Types[abim.Name]
			if ok {
				ctr.SafeMethods[len(ctr.SafeMethods)-1].ExtendedReturn = et
				if abim.ReturnType == smartcontract.ArrayType && len(et.Name) > 0 {
					ctr.SafeMethods[len(ctr.SafeMethods)-1].ItemTo = cutPointer(ctr.Methods[i].ReturnType)
				}
			}
			ctr.Methods = slices.Delete(ctr.Methods, i, i+1)
			i--
		} else {
			ctr.Methods[i].Comment = fmt.Sprintf("creates a transaction invoking `%s` method of the contract.", ctr.Methods[i].NameABI)
			if ctr.Methods[i].ReturnType == "bool" {
				imports["github.com/nspcc-dev/neo-go/pkg/smartcontract"] = struct{}{}
			}
		}
	}
	for _, et := range cfg.NamedTypes {
		addETImports(et, cfg.NamedTypes, imports)
	}
	if len(cfg.NamedTypes) > 0 {
		imports["errors"] = struct{}{}
	}
	for _, abiEvent := range cfg.Manifest.ABI.Events {
		eBindingName := ToEventBindingName(abiEvent.Name)
		eTmp := CustomEventTemplate{
			Name:         eBindingName,
			ManifestName: abiEvent.Name,
		}
		for i := range abiEvent.Parameters {
			pBindingName := ToParameterBindingName(abiEvent.Parameters[i].Name)
			fullPName := eBindingName + "." + pBindingName
			typeStr, pkg := scTypeConverter(fullPName, abiEvent.Parameters[i].Type, &cfg)
			if pkg != "" {
				imports[pkg] = struct{}{}
			}

			var (
				extType binding.ExtendedType
				ok      bool
			)
			if extType, ok = cfg.Types[fullPName]; !ok {
				extType = binding.ExtendedType{
					Base: abiEvent.Parameters[i].Type,
				}
				addETImports(extType, cfg.NamedTypes, imports)
			}
			eTmp.Parameters = append(eTmp.Parameters, EventParamTmpl{
				ParamTmpl: binding.ParamTmpl{
					Name: pBindingName,
					Type: typeStr,
				},
				ExtType: extType,
			})
		}
		ctr.CustomEvents = append(ctr.CustomEvents, eTmp)
	}

	if len(ctr.CustomEvents) > 0 {
		imports["github.com/nspcc-dev/neo-go/pkg/neorpc/result"] = struct{}{}
		imports["github.com/nspcc-dev/neo-go/pkg/vm/stackitem"] = struct{}{}
		imports["fmt"] = struct{}{}
		imports["errors"] = struct{}{}
	}

	for i := range ctr.SafeMethods {
		switch ctr.SafeMethods[i].ReturnType {
		case "any":
			abim := cfg.Manifest.ABI.GetMethod(ctr.SafeMethods[i].NameABI, len(ctr.SafeMethods[i].Arguments))
			if abim.ReturnType == smartcontract.InteropInterfaceType {
				imports["github.com/google/uuid"] = struct{}{}
				imports["github.com/nspcc-dev/neo-go/pkg/vm/stackitem"] = struct{}{}
				imports["github.com/nspcc-dev/neo-go/pkg/neorpc/result"] = struct{}{}
				ctr.SafeMethods[i].ReturnType = "uuid.UUID, result.Iterator"
				ctr.SafeMethods[i].Unwrapper = "SessionIterator"
				ctr.HasIterator = true
			} else {
				imports["github.com/nspcc-dev/neo-go/pkg/vm/stackitem"] = struct{}{}
				ctr.SafeMethods[i].ReturnType = "any"
				ctr.SafeMethods[i].Unwrapper = "Item"
			}
		case "bool":
			ctr.SafeMethods[i].Unwrapper = "Bool"
		case "*big.Int":
			ctr.SafeMethods[i].Unwrapper = "BigInt"
		case "string":
			ctr.SafeMethods[i].Unwrapper = "UTF8String"
		case "util.Uint160":
			ctr.SafeMethods[i].Unwrapper = "Uint160"
		case "util.Uint256":
			ctr.SafeMethods[i].Unwrapper = "Uint256"
		case "*keys.PublicKey":
			ctr.SafeMethods[i].Unwrapper = "PublicKey"
		case "[]byte":
			ctr.SafeMethods[i].Unwrapper = "Bytes"
		case "[]any":
			imports["github.com/nspcc-dev/neo-go/pkg/vm/stackitem"] = struct{}{}
			ctr.SafeMethods[i].ReturnType = "[]stackitem.Item"
			ctr.SafeMethods[i].Unwrapper = "Array"
		case "*stackitem.Map":
			ctr.SafeMethods[i].Unwrapper = "Map"
		case "[]bool":
			ctr.SafeMethods[i].Unwrapper = "ArrayOfBools"
		case "[]*big.Int":
			ctr.SafeMethods[i].Unwrapper = "ArrayOfBigInts"
		case "[][]byte":
			ctr.SafeMethods[i].Unwrapper = "ArrayOfBytes"
		case "[]string":
			ctr.SafeMethods[i].Unwrapper = "ArrayOfUTF8Strings"
		case "[]util.Uint160":
			ctr.SafeMethods[i].Unwrapper = "ArrayOfUint160"
		case "[]util.Uint256":
			ctr.SafeMethods[i].Unwrapper = "ArrayOfUint256"
		case "keys.PublicKeys":
			ctr.SafeMethods[i].Unwrapper = "ArrayOfPublicKeys"
		default:
			addETImports(ctr.SafeMethods[i].ExtendedReturn, cfg.NamedTypes, imports)
			ctr.SafeMethods[i].Unwrapper = "Item"
		}
	}

	imports["github.com/nspcc-dev/neo-go/pkg/util"] = struct{}{}
	if len(ctr.SafeMethods) > 0 {
		imports["github.com/nspcc-dev/neo-go/pkg/rpcclient/unwrap"] = struct{}{}
		if !(ctr.IsNep17 || ctr.IsNep11D || ctr.IsNep11ND) {
			imports["github.com/nspcc-dev/neo-go/pkg/neorpc/result"] = struct{}{}
		}
	}
	if len(ctr.Methods) > 0 {
		imports["github.com/nspcc-dev/neo-go/pkg/core/transaction"] = struct{}{}
	}
	if len(ctr.Methods) > 0 || ctr.IsNep17 || ctr.IsNep11D || ctr.IsNep11ND {
		ctr.HasWriter = true
	}
	if len(ctr.SafeMethods) > 0 || ctr.IsNep17 || ctr.IsNep11D || ctr.IsNep11ND {
		ctr.HasReader = true
	}
	ctr.Imports = ctr.Imports[:0]
	for imp := range imports {
		ctr.Imports = append(ctr.Imports, imp)
	}
	slices.Sort(ctr.Imports)
	return ctr
}

func addETImports(et binding.ExtendedType, named map[string]binding.ExtendedType, imports map[string]struct{}) {
	_, pkg := extendedTypeToGo(et, named)
	if pkg != "" {
		imports[pkg] = struct{}{}
	}
	// Additional packages used during decoding.
	switch et.Base {
	case smartcontract.StringType:
		imports["unicode/utf8"] = struct{}{}
		imports["errors"] = struct{}{}
	case smartcontract.PublicKeyType:
		imports["crypto/elliptic"] = struct{}{}
	case smartcontract.MapType:
		imports["fmt"] = struct{}{}
	case smartcontract.ArrayType:
		imports["errors"] = struct{}{}
		imports["fmt"] = struct{}{}
	}
	if et.Value != nil {
		addETImports(*et.Value, named, imports)
	}
	if et.Base == smartcontract.MapType {
		addETImports(binding.ExtendedType{Base: et.Key}, named, imports)
	}
	for i := range et.Fields {
		addETImports(et.Fields[i].ExtendedType, named, imports)
	}
}

func cutPointer(s string) string {
	if s[0] == '*' {
		return s[1:]
	}
	return s
}

func toTypeName(s string) string {
	return strings.Map(func(c rune) rune {
		if c == '.' {
			return -1
		}
		return c
	}, upperFirst(s))
}

func addIndent(str string, ind string) string {
	return strings.ReplaceAll(str, "\n", "\n"+ind)
}

// ToEventBindingName converts event name specified in the contract manifest to
// a valid go exported event structure name.
func ToEventBindingName(eventName string) string {
	return toPascalCase(eventName) + "Event"
}

// ToParameterBindingName converts parameter name specified in the contract
// manifest to a valid go structure's exported field name.
func ToParameterBindingName(paramName string) string {
	return toPascalCase(paramName)
}

// toPascalCase removes all non-unicode characters from the provided string and
// converts it to pascal case using space as delimiter.
func toPascalCase(s string) string {
	var res string
	ss := strings.Split(s, " ")
	for i := range ss { // TODO: use DecodeRuneInString instead.
		var word string
		for _, ch := range ss[i] {
			var ok bool
			if len(res) == 0 && len(word) == 0 {
				ok = unicode.IsLetter(ch)
			} else {
				ok = unicode.IsLetter(ch) || unicode.IsDigit(ch) || ch == '_'
			}
			if ok {
				word += string(ch)
			}
		}
		if len(word) > 0 {
			res += upperFirst(word)
		}
	}
	return res
}

func upperFirst(s string) string {
	return strings.ToUpper(s[0:1]) + s[1:]
}
