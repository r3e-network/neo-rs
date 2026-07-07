//! Small Prometheus text-format writers for bounded-label metrics.

pub(super) fn push_single_label_metric(
    output: &mut String,
    metric: &str,
    label_name: &str,
    label_value: &str,
    value: u64,
) {
    output.push_str(metric);
    output.push('{');
    output.push_str(label_name);
    output.push_str("=\"");
    output.push_str(label_value);
    output.push_str("\"} ");
    output.push_str(&value.to_string());
    output.push('\n');
}

pub(super) fn push_native_hook_metric(
    output: &mut String,
    metric: &str,
    trigger: &str,
    contract: &str,
    contract_id: i32,
    value: u64,
) {
    output.push_str(metric);
    output.push_str("{trigger=\"");
    output.push_str(trigger);
    output.push_str("\",contract=\"");
    output.push_str(contract);
    output.push_str("\",id=\"");
    output.push_str(&contract_id.to_string());
    output.push_str("\"} ");
    output.push_str(&value.to_string());
    output.push('\n');
}
