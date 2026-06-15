# IronCRM Odoo Sandbox

Start a local Odoo CRM sandbox:

```sh
docker compose -f dev/odoo/docker-compose.yml up -d
```

Odoo web UI:

```text
http://127.0.0.1:8069
```

IronCRM import connection:

```text
postgres://odoo:odoo@127.0.0.1:5433/odoo
```

The first `up` runs a one-shot `odoo-init` container that creates the `odoo` database and installs the CRM module with demo data enabled.
