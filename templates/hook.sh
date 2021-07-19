#!/bin/bash

set -euo pipefail

{% for hook in hooks %}
{{ hook.name }}() {
    true
}
{% endfor %}

main() {
    true
}

main
