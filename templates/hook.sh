#!/bin/bash

set -euo pipefail

{% for hook in hooks %}
{{ hook.name }}() {
    {{ hook.command }}
}
{% endfor %}

main() {
    {% for hook in hooks %}
    {{ hook.name }}
    {% endfor %}
}

main
