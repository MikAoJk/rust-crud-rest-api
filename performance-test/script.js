import http from 'k6/http';
import { sleep, check, fail } from 'k6';
import exec from 'k6/execution';

export const options = {
  vus: 10,
  duration: '30s',
  thresholds: {
    checks: ['rate==1'],
    http_req_failed: ['rate==0'],
  },
};

const baseUrl = (__ENV.BASE_URL || 'http://localhost:8080').replace(/\/+$/, '');

function params(operation, name) {
  return {
    headers: { 'Content-Type': 'application/json' },
    tags: { operation, name },
  };
}

function expect(response, label, status, body) {
  const checks = { [`${label}: status is ${status}`]: (res) => res.status === status };
  if (body !== undefined) {
    checks[`${label}: response body matches`] = (res) => res.body === body;
  }
  if (!check(response, checks)) {
    fail(`${label} failed: status=${response.status}, body=${response.body}`);
  }
}

function json(response, label) {
  try {
    return response.json();
  } catch (error) {
    check(response, { [`${label}: valid JSON`]: () => false });
    fail(`${label} returned invalid JSON: ${error.message}`);
  }
}

function verifyUser(response, expected, label) {
  expect(response, label, 200);
  const user = json(response, label);
  if (!check(user, {
    [`${label}: user matches`]: (value) => value !== null
      && value.id === expected.id
      && value.name === expected.name
      && value.email === expected.email,
  })) {
    fail(`${label} returned unexpected user data: ${response.body}`);
  }
}

export default function() {
  const unique = `${Date.now()}-${exec.vu.idInTest}-${exec.scenario.iterationInTest}-${Math.random().toString(16).slice(2)}`;
  const user = { name: `k6-${unique}`, email: `k6-${unique}@example.com` };
  let id;

  try {
    const created = http.post(`${baseUrl}/users`, JSON.stringify(user), params('create', 'POST /users'));
    expect(created, 'create', 200, 'User created');

    // POST returns a message, so discover the ID using this iteration's unique email.
    const listed = http.get(`${baseUrl}/users`, params('list', 'GET /users'));
    expect(listed, 'list', 200);
    const users = json(listed, 'list');
    const matches = Array.isArray(users)
      ? users.filter((value) => value !== null && value.email === user.email)
      : [];
    if (!check(matches, {
      'list: created user has a unique ID': (values) => values.length === 1
        && Number.isInteger(values[0].id) && values[0].id > 0,
    })) {
      fail('Could not identify the created user in GET /users');
    }
    id = matches[0].id;
    const url = `${baseUrl}/users/${id}`;

    verifyUser(http.get(url, params('read', 'GET /users/:id')), { ...user, id }, 'read');

    const updatedUser = { name: `updated-${unique}`, email: `updated-${unique}@example.com` };
    const updated = http.put(url, JSON.stringify(updatedUser), params('update', 'PUT /users/:id'));
    expect(updated, 'update', 200, 'User updated');
    verifyUser(http.get(url, params('read_updated', 'GET /users/:id')),
      { ...updatedUser, id }, 'read updated');
  } finally {
    try {
      if (id !== undefined) {
        const url = `${baseUrl}/users/${id}`;
        expect(http.del(url, null, params('delete', 'DELETE /users/:id')),
          'delete', 200, 'User deleted');
        const deletedParams = params('read_deleted', 'GET /users/:id');
        deletedParams.responseCallback = http.expectedStatuses(404);
        expect(http.get(url, deletedParams), 'read deleted', 404, 'User not found');
      }
    } finally {
      sleep(1);
    }
  }
}
