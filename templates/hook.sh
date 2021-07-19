#!/bin/bash

set -euo pipefail

{% for hook in hooks %}
{{ hook.name }}() {
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
