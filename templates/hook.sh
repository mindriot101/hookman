#!/bin/bash

set -euo pipefail

{% for hook in hooks %}
{{ hook.name }}() {
    {% match hook.original_name %}
    {% when Some with (original_name) %}
    echo "Running: {{ original_name }}" >&2
    {% when None %}
    {% endmatch %}
    {{ hook.command }}
}
{% endfor %}

main() {
    {% for hook in hooks %}
    {% if hook.background %}
    {{ hook.name }} &
    {% else %}
    {{ hook.name }}
    {% endif %}
    {% endfor %}
}

main
