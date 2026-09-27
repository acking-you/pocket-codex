import 'package:flutter/material.dart';

/// Collects native OpenCode form values without following external actions.
class OpenCodeFormDialog extends StatefulWidget {
  const OpenCodeFormDialog({super.key, required this.request});

  final Map<String, dynamic> request;

  @override
  State<OpenCodeFormDialog> createState() => _OpenCodeFormDialogState();
}

class _OpenCodeFormDialogState extends State<OpenCodeFormDialog> {
  late final fields = (widget.request['fields'] as List? ?? [])
      .whereType<Map>()
      .map((field) => Map<String, dynamic>.from(field))
      .toList(growable: false);
  final inputs = <String, TextEditingController>{};
  final customInputs = <String, TextEditingController>{};
  final values = <String, dynamic>{};
  Map<String, String> errors = {};

  String text(String en, String zh) =>
      Localizations.localeOf(context).languageCode == 'zh' ? zh : en;

  @override
  void initState() {
    super.initState();
    for (final field in fields) {
      final key = '${field['key']}';
      final initial = field['default'];
      if (['string', 'number', 'integer'].contains(field['type'])) {
        inputs[key] = TextEditingController(text: initial?.toString() ?? '');
      } else if (initial != null) {
        values[key] = initial is List ? List<dynamic>.from(initial) : initial;
      }
      if (field['type'] == 'multiselect' && field['custom'] == true) {
        customInputs[key] = TextEditingController();
      }
    }
  }

  @override
  void dispose() {
    for (final input in [...inputs.values, ...customInputs.values]) {
      input.dispose();
    }
    super.dispose();
  }

  bool get supported =>
      fields.isNotEmpty &&
      fields.length == (widget.request['fields'] as List).length &&
      fields.every(
        (field) =>
            field['key'] is String &&
            !field.containsKey('pattern') &&
            !field.containsKey('format') &&
            [
              'string',
              'number',
              'integer',
              'boolean',
              'multiselect',
            ].contains(field['type']),
      );

  bool _active(Map<String, dynamic> field, Map<String, dynamic> answers) =>
      (field['when'] as List? ?? []).every((condition) {
        if (condition is! Map || !answers.containsKey(condition['key'])) {
          return false;
        }
        final value = answers[condition['key']];
        final matches = value is List
            ? value.contains(condition['value'])
            : value == condition['value'];
        return switch (condition['op']) {
          'eq' => matches,
          'neq' => !matches,
          _ => false,
        };
      });

  dynamic _value(Map<String, dynamic> field) {
    final key = '${field['key']}';
    if (field['type'] == 'multiselect' && customInputs.containsKey(key)) {
      return <String>{
        ...(values[key] as List? ?? []).whereType<String>(),
        ...customInputs[key]!.text
            .split('\n')
            .map((item) => item.trim())
            .where((item) => item.isNotEmpty),
      }.toList();
    }
    final input = inputs[key];
    if (input == null) return values[key];
    if (input.text.isEmpty) {
      return field['type'] == 'string' && field['default'] == '' ? '' : null;
    }
    if (field['type'] == 'string') return input.text;
    final number = num.tryParse(input.text);
    if (number == null || !number.isFinite) return input.text;
    if (field['type'] == 'integer' && number == number.roundToDouble()) {
      final integer = number.toInt();
      if (integer.toDouble() == number) return integer;
    }
    return number;
  }

  Map<String, dynamic> _answers() {
    final answers = <String, dynamic>{};
    for (final field in fields) {
      if (!_active(field, answers)) continue;
      final value = _value(field);
      if (value != null) answers['${field['key']}'] = value;
    }
    return answers;
  }

  String? _validate(Map<String, dynamic> field, dynamic value) {
    if (value == null) {
      return field['required'] == true ? text('Required', '必填') : null;
    }
    if (field['required'] == true &&
        (value == '' || value is List && value.isEmpty)) {
      return text('Required', '必填');
    }
    if (['number', 'integer'].contains(field['type']) &&
        (value is! num ||
            !value.isFinite ||
            field['type'] == 'integer' &&
                value is! int &&
                value != value.roundToDouble())) {
      return text('Enter a valid number', '请输入有效数字');
    }
    final invalid = text('Invalid value', '值不符合要求');
    if (value is num) {
      if (!value.isFinite ||
          field['minimum'] is num && value < field['minimum'] ||
          field['maximum'] is num && value > field['maximum']) {
        return invalid;
      }
    }
    if (field['type'] == 'string') {
      if (value is! String) return invalid;
      if (field['minLength'] is num && value.length < field['minLength'] ||
          field['maxLength'] is num && value.length > field['maxLength']) {
        return invalid;
      }
      if (field['options'] is List &&
          field['custom'] != true &&
          !(field['options'] as List).whereType<Map>().any(
            (option) => option['value'] == value,
          )) {
        return invalid;
      }
    }
    if (field['type'] == 'boolean' && value is! bool) return invalid;
    if (field['type'] == 'multiselect') {
      if (value is! List || value.any((item) => item is! String)) {
        return invalid;
      }
      if (field['minItems'] is num && value.length < field['minItems'] ||
          field['maxItems'] is num && value.length > field['maxItems']) {
        return invalid;
      }
      final options = (field['options'] as List? ?? []).whereType<Map>().map(
        (option) => option['value'],
      );
      if (field['custom'] != true &&
          value.any((item) => !options.contains(item))) {
        return invalid;
      }
    }
    return null;
  }

  void _submit() {
    final answers = _answers();
    final invalid = <String, String>{};
    for (final field in fields) {
      if (!_active(field, answers)) continue;
      final error = _validate(field, answers[field['key']]);
      if (error != null) invalid['${field['key']}'] = error;
    }
    if (invalid.isNotEmpty) {
      setState(() => errors = invalid);
      return;
    }
    Navigator.pop(context, answers);
  }

  @override
  Widget build(BuildContext context) {
    final answers = _answers();
    return AlertDialog(
      title: Text('${widget.request['title'] ?? text('Answer form', '回答表单')}'),
      content: SizedBox(
        width: 440,
        child: SingleChildScrollView(
          child: supported
              ? Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    for (final field in fields)
                      if (field['hidden'] != true && _active(field, answers))
                        Padding(
                          padding: const EdgeInsets.only(bottom: 16),
                          child: _field(field),
                        ),
                    for (final field in fields)
                      if (field['hidden'] == true &&
                          errors[field['key']] != null)
                        Text(
                          '${field['title'] ?? field['key']}: ${errors[field['key']]}',
                        ),
                  ],
                )
              : Text(
                  text(
                    'This form has unsupported fields or constraints. Complete it in OpenCode.',
                    '此表单包含尚不支持的字段、校验规则或外部操作，请在 OpenCode 中处理。',
                  ),
                ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(text('Cancel', '取消')),
        ),
        FilledButton(
          onPressed: supported ? _submit : null,
          child: Text(text('Submit answers', '提交回答')),
        ),
      ],
    );
  }

  Widget _field(Map<String, dynamic> field) {
    final key = '${field['key']}';
    final title = '${field['title'] ?? key}';
    if (field['type'] == 'boolean') {
      return CheckboxListTile(
        key: Key('opencode-form-$key'),
        contentPadding: EdgeInsets.zero,
        tristate: true,
        value: values[key] as bool?,
        title: Text(title),
        subtitle: errors[key] != null ? Text(errors[key]!) : null,
        onChanged: (value) => setState(() => values[key] = value),
      );
    }
    if (field['type'] == 'multiselect') {
      final selected = (values[key] as List? ?? [])
          .whereType<String>()
          .toList();
      final options = (field['options'] as List? ?? [])
          .whereType<Map>()
          .toList();
      final defaults = field['custom'] == true
          ? (field['default'] as List? ?? []).whereType<String>().where(
              (value) => !options.any((option) => option['value'] == value),
            )
          : const <String>[];
      return Column(
        key: Key('opencode-form-$key'),
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title),
          for (final option in [
            ...options,
            for (final value in defaults) {'value': value, 'label': value},
          ])
            CheckboxListTile(
              contentPadding: EdgeInsets.zero,
              title: Text('${option['label']}'),
              subtitle: option['description'] is String
                  ? Text(option['description'] as String)
                  : null,
              value: selected.contains(option['value']),
              onChanged: (checked) => setState(() {
                if (checked == true) {
                  selected.add('${option['value']}');
                } else {
                  selected.remove(option['value']);
                }
                values[key] = selected;
              }),
            ),
          if (customInputs.containsKey(key))
            TextField(
              key: Key('opencode-form-custom-$key'),
              controller: customInputs[key],
              minLines: 1,
              maxLines: 3,
              decoration: InputDecoration(
                labelText: text('Custom values', '自定义值'),
              ),
              onChanged: (_) => setState(() {}),
            ),
          if (errors[key] != null) Text(errors[key]!),
        ],
      );
    }
    if (field['type'] == 'string' && field['options'] is List) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title),
          RadioGroup<String>(
            groupValue: inputs[key]!.text,
            onChanged: (value) =>
                setState(() => inputs[key]!.text = value ?? ''),
            child: Column(
              children: [
                for (final option
                    in (field['options'] as List).whereType<Map>())
                  RadioListTile<String>(
                    contentPadding: EdgeInsets.zero,
                    value: '${option['value']}',
                    title: Text('${option['label']}'),
                    subtitle: option['description'] is String
                        ? Text(option['description'] as String)
                        : null,
                  ),
              ],
            ),
          ),
          if (field['custom'] == true) _textField(field),
          if (field['custom'] != true && errors[key] != null)
            Text(errors[key]!),
        ],
      );
    }
    return _textField(field);
  }

  Widget _textField(Map<String, dynamic> field) {
    final key = '${field['key']}';
    return TextField(
      key: Key('opencode-form-$key'),
      controller: inputs[key],
      keyboardType: field['type'] == 'string'
          ? TextInputType.text
          : const TextInputType.numberWithOptions(decimal: true, signed: true),
      decoration: InputDecoration(
        labelText: '${field['title'] ?? key}',
        hintText: field['placeholder'] as String?,
        helperText: field['description'] as String?,
        errorText: errors[key],
      ),
      onChanged: (_) => setState(() {}),
    );
  }
}
